use crate::catalog::catalog_manager::CatalogManager;
use crate::catalog::schema::Schema;
use crate::catalog::column::Column;
use crate::types::{Value, TypeId};
use crate::execution::executor::{Executor, ExecutorContext};
use crate::execution::insert::InsertExecutor;
use crate::execution::seq_scan::SeqScanExecutor;
use crate::execution::index_scan::IndexScanExecutor;
use crate::execution::expression::{Expression, BinaryOperator};
use crate::execution::filter::FilterExecutor;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use sqlparser::ast::{Statement, ObjectName, DataType, SetExpr, Expr, Values, ColumnOption};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SQLError {
    #[error("Parse Error: {0}")]
    ParseError(String),
    #[error("Catalog Error: {0}")]
    CatalogError(String),
    #[error("Execution Error: {0}")]
    ExecutionError(String),
    #[error("Unsupported feature: {0}")]
    Unsupported(String),
}

pub struct SQLEngine {
    pub catalog: CatalogManager,
}

impl SQLEngine {
    pub fn new(catalog: CatalogManager) -> Self {
        Self { catalog }
    }

    pub fn execute(&mut self, sql: &str) -> Result<String, SQLError> {
        let dialect = GenericDialect {};
        let ast = Parser::parse_sql(&dialect, sql)
            .map_err(|e| SQLError::ParseError(e.to_string()))?;

        let mut output = String::new();

        for statement in ast {
            match statement {
                Statement::CreateTable { name, columns, .. } => {
                    self.handle_create_table(name, columns)?;
                    output.push_str("Table created.\n");
                }
                Statement::Insert { table_name, source, .. } => {
                    let count = self.handle_insert(table_name, source)?;
                    output.push_str(&format!("Inserted {} rows.\n", count));
                }
                Statement::Query(query) => {
                    let result = self.handle_query(query)?;
                    output.push_str(&result);
                }
                _ => return Err(SQLError::Unsupported("Only CREATE, INSERT, and SELECT are supported".into())),
            }
        }

        Ok(output)
    }

    fn handle_create_table(&mut self, name: ObjectName, columns: Vec<sqlparser::ast::ColumnDef>) -> Result<(), SQLError> {
        let table_name = name.to_string();
        let mut schema_cols = Vec::new();

        for col in columns {
            let col_name = col.name.to_string();
            let type_id = match col.data_type {
                DataType::Int(_) | DataType::Integer(_) => TypeId::Integer,
                DataType::Boolean => TypeId::Boolean,
                DataType::Varchar(_) | DataType::String => TypeId::Varchar,
                _ => return Err(SQLError::Unsupported(format!("Data type {:?} not supported", col.data_type))),
            };

            let mut is_primary = false;
            for option in col.options {
                if let ColumnOption::Unique { is_primary: true } = option.option {
                    is_primary = true;
                }
            }

            schema_cols.push(Column::new_with_primary(col_name, type_id, is_primary));
        }

        let schema = Schema::new(schema_cols);
        self.catalog.create_table(table_name, schema);
        Ok(())
    }

    fn handle_insert(&mut self, table_name: ObjectName, source: Box<sqlparser::ast::Query>) -> Result<usize, SQLError> {
        let name_str = table_name.to_string();
        let table_meta = self.catalog.get_table(&name_str)
            .ok_or_else(|| SQLError::CatalogError(format!("Table {} not found", name_str)))?;

        // Extract values from the AST
        let rows = match *source.body {
            SetExpr::Values(Values { rows, .. }) => rows,
            _ => return Err(SQLError::Unsupported("Only INSERT INTO ... VALUES (...) supported".into())),
        };

        let mut values_batch = Vec::new();

        for row in rows {
            let mut row_values = Vec::new();
            if row.len() != table_meta.schema.column_count() {
                 return Err(SQLError::ExecutionError(format!("Column count mismatch. Expected {}, got {}", table_meta.schema.column_count(), row.len())));
            }

            for (i, expr) in row.iter().enumerate() {
                let target_type = table_meta.schema.columns[i].type_id;
                let val = match (expr, target_type) {
                    (Expr::Value(v), TypeId::Integer) => match v {
                         sqlparser::ast::Value::Number(n, _) => Value::Integer(n.parse().unwrap_or(0)),
                         _ => return Err(SQLError::ExecutionError(format!("Expected Integer at index {}", i))),
                    },
                    (Expr::Value(v), TypeId::Boolean) => match v {
                        sqlparser::ast::Value::Boolean(b) => Value::Boolean(*b),
                        _ => return Err(SQLError::ExecutionError(format!("Expected Boolean at index {}", i))),
                    },
                    (Expr::Value(v), TypeId::Varchar) => match v {
                        sqlparser::ast::Value::SingleQuotedString(s) => Value::Varchar(s.clone()),
                        _ => return Err(SQLError::ExecutionError(format!("Expected String at index {}", i))),
                    },
                    _ => return Err(SQLError::ExecutionError(format!("Type mismatch or unsupported value at index {}", i))),
                };
                row_values.push(val);
            }
            values_batch.push(row_values);
        }

        let count = values_batch.len();
        let context = ExecutorContext { catalog: table_meta.clone() };
        let mut exec = InsertExecutor::new(&context, values_batch);
        exec.init();
        
        while exec.next().is_some() {}
        
        Ok(count)
    }

    fn handle_query(&mut self, query: Box<sqlparser::ast::Query>) -> Result<String, SQLError> {
        // Assume SELECT * FROM table
        let (table_name, selection) = match query.body.as_ref() {
            SetExpr::Select(select) => {
                 let table_name = match select.from.first() {
                     Some(table_with_joins) => match &table_with_joins.relation {
                         sqlparser::ast::TableFactor::Table { name, .. } => name.to_string(),
                         _ => return Err(SQLError::Unsupported("Only SELECT FROM table supported".into())),
                     },
                     None => return Err(SQLError::Unsupported("No table specified".into())),
                 };
                 (table_name, select.selection.clone())
            },
            _ => return Err(SQLError::Unsupported("Only SELECT queries supported".into())),
        };

        let table_meta = self.catalog.get_table(&table_name)
            .ok_or_else(|| SQLError::CatalogError(format!("Table {} not found", table_name)))?;
        
        let context = ExecutorContext { catalog: table_meta.clone() };
        
        // Build Executor Chain
        let mut root_exec: Box<dyn Executor>;

        if let Some(expr) = selection {
            let predicate = self.parse_expression(expr, &table_meta.schema)?;
            
            // Optimization: Check for Index Scan
            // Logic: if predicate is "col = val" AND col is primary key (idx 0) AND table has index
            let mut use_index = false;
            let mut key_val = 0;

            if table_meta.index.is_some() {
                 if let Expression::Binary { left, op, right } = &predicate {
                     if *op == BinaryOperator::Eq {
                         if let (Expression::Column(idx), Expression::Constant(val)) = (left.as_ref(), right.as_ref()) {
                             if *idx == 0 { // Primary Key / First Column
                                 if let Value::Integer(k) = val {
                                     use_index = true;
                                     key_val = *k;
                                 }
                             }
                         }
                     }
                 }
            }

            if use_index {
                // println!("Using Index Scan for key: {}", key_val);
                root_exec = Box::new(IndexScanExecutor::new(&context, key_val));
            } else {
                // Fallback to SeqScan with Zone Maps
                let mut scan_executor = SeqScanExecutor::new(&context);
                scan_executor.set_predicate(predicate.clone());
                
                let scan_exec_box: Box<dyn Executor> = Box::new(scan_executor);
                root_exec = Box::new(FilterExecutor::new(&context, scan_exec_box, predicate));
            }
        } else {
             root_exec = Box::new(SeqScanExecutor::new(&context));
        }
        
        root_exec.init();

        let mut output = String::new();
        // Header
        for col in &table_meta.schema.columns {
            output.push_str(&format!("{:15} | ", col.name));
        }
        output.push_str("\n");
        output.push_str(&"-".repeat(table_meta.schema.columns.len() * 18));
        output.push_str("\n");

        while let Some(tuple) = root_exec.next() {
            for i in 0..table_meta.schema.column_count() {
                let val = tuple.get_value(&table_meta.schema, i);
                match val {
                    Value::Integer(v) => output.push_str(&format!("{:15} | ", v)),
                    Value::Boolean(v) => output.push_str(&format!("{:15} | ", v)),
                    Value::Varchar(v) => output.push_str(&format!("{:15} | ", v)),
                }
            }
            output.push_str("\n");
        }

        Ok(output)
    }

    fn parse_expression(&self, expr: Expr, schema: &Schema) -> Result<Expression, SQLError> {
        match expr {
            Expr::BinaryOp { left, op, right } => {
                let l_expr = self.parse_expression(*left, schema)?;
                let r_expr = self.parse_expression(*right, schema)?;
                
                let bin_op = match op {
                    sqlparser::ast::BinaryOperator::Eq => BinaryOperator::Eq,
                    sqlparser::ast::BinaryOperator::NotEq => BinaryOperator::NotEq,
                    sqlparser::ast::BinaryOperator::Lt => BinaryOperator::Lt,
                    sqlparser::ast::BinaryOperator::Gt => BinaryOperator::Gt,
                    sqlparser::ast::BinaryOperator::LtEq => BinaryOperator::LtEq,
                    sqlparser::ast::BinaryOperator::GtEq => BinaryOperator::GtEq,
                    _ => return Err(SQLError::Unsupported(format!("Binary Operator {:?} not supported", op))),
                };
                
                Ok(Expression::Binary {
                    left: Box::new(l_expr),
                    op: bin_op,
                    right: Box::new(r_expr),
                })
            }
            Expr::Identifier(ident) => {
                let name = ident.to_string();
                let idx = schema.get_col_index(&name)
                    .ok_or_else(|| SQLError::ParseError(format!("Column {} not found", name)))?;
                Ok(Expression::Column(idx))
            }
            Expr::Value(v) => {
                 match v {
                     sqlparser::ast::Value::Number(n, _) => Ok(Expression::Constant(Value::Integer(n.parse().unwrap_or(0)))),
                     sqlparser::ast::Value::Boolean(b) => Ok(Expression::Constant(Value::Boolean(b))),
                     sqlparser::ast::Value::SingleQuotedString(s) => Ok(Expression::Constant(Value::Varchar(s))),
                     _ => Err(SQLError::Unsupported(format!("Value {:?} not supported in expression", v))),
                 }
            }
            _ => Err(SQLError::Unsupported(format!("Expression {:?} not supported", expr))),
        }
    }
}
