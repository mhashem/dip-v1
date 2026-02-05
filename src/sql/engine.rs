use crate::catalog::catalog_manager::CatalogManager;
use crate::catalog::column::Column;
use crate::catalog::schema::Schema;
use crate::concurrency::transaction::Transaction;
use crate::concurrency::transaction_manager::TransactionManager;
use crate::errors::DipError;
use crate::execution::executor::{Executor, ExecutorContext};
use crate::execution::expression::{BinaryOperator, Expression};
use crate::execution::filter::FilterExecutor;
use crate::execution::index_scan::IndexScanExecutor;
use crate::execution::insert::InsertExecutor;
use crate::execution::seq_scan::SeqScanExecutor;
use crate::storage::index::b_plus_tree::BPlusTree;
use crate::types::{TypeId, Value};
use sqlparser::ast::{ColumnOption, DataType, Expr, ObjectName, SetExpr, Statement, Values};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use std::sync::{Arc, Mutex, RwLock};

pub struct SQLEngine {
    pub catalog: CatalogManager,
    pub transaction_manager: Arc<TransactionManager>,
    pub current_txn: Option<Arc<Mutex<Transaction>>>,
}

impl SQLEngine {
    pub fn new(catalog: CatalogManager) -> Self {
        Self {
            catalog,
            transaction_manager: Arc::new(TransactionManager::new()),
            current_txn: None,
        }
    }

    pub fn execute(&mut self, sql: &str) -> Result<String, DipError> {
        let dialect = GenericDialect {};
        let ast = Parser::parse_sql(&dialect, sql)
            .map_err(|e| DipError::ParseError(e.to_string()))?;

        let mut output = String::new();
        let was_in_txn = self.current_txn.is_some();
        let mut started_explicit_txn = false;
        
        for statement in ast {
            if matches!(statement, Statement::StartTransaction { .. }) {
                started_explicit_txn = true;
            }

            let txn = if let Some(t) = &self.current_txn {
                t.clone()
            } else {
                let t = self.transaction_manager.begin();
                if !matches!(statement, Statement::StartTransaction { .. }) {
                    self.current_txn = Some(t.clone());
                }
                t
            };

            let result = match statement {
                Statement::StartTransaction { .. } => {
                    if was_in_txn {
                        return Err(DipError::Unsupported("Nested transactions not supported".into()));
                    }
                    self.current_txn = Some(txn.clone());
                    Ok("Transaction started.\n".to_string())
                }
                Statement::Commit { .. } => {
                    if self.current_txn.is_none() {
                        return Err(DipError::Unsupported("No active transaction to commit".into()));
                    }
                    self.transaction_manager.commit(txn.clone());
                    self.current_txn = None;
                    Ok("Transaction committed.\n".to_string())
                }
                Statement::Rollback { .. } => {
                    if self.current_txn.is_none() {
                        return Err(DipError::Unsupported("No active transaction to rollback".into()));
                    }
                    self.rollback(txn.clone());
                    self.transaction_manager.abort(txn.clone());
                    self.current_txn = None;
                    Ok("Transaction rolled back.\n".to_string())
                }
                Statement::CreateTable { name, columns, .. } => {
                    self.handle_create_table(name, columns).map(|_| "Table created.\n".to_string())
                }
                Statement::Insert { table_name, source, .. } => {
                    self.handle_insert(table_name, source, txn.clone())
                        .map(|count| format!("Inserted {} rows.\n", count))
                }
                Statement::Query(query) => {
                    self.handle_query(query, txn.clone())
                }
                Statement::Update { table, assignments, selection, .. } => {
                    self.handle_update(table, assignments, selection, txn.clone())
                        .map(|count| format!("Updated {} rows.\n", count))
                }
                Statement::Delete { tables, selection, from, .. } => {
                    self.handle_delete(tables, selection, from, txn.clone())
                        .map(|count| format!("Deleted {} rows.\n", count))
                }
                Statement::CreateIndex { table_name, columns, .. } => {
                    self.handle_create_index(table_name, columns).map(|_| "Index created.\n".to_string())
                }
                _ => Err(DipError::Unsupported("Unsupported statement".into())),
            };

            match result {
                Ok(msg) => {
                    output.push_str(&msg);
                }
                Err(e) => {
                    // ABORT AND ROLLBACK
                    self.rollback(txn.clone());
                    self.transaction_manager.abort(txn);
                    self.current_txn = None;
                    return Err(e);
                }
            }
        }

        // Auto-commit if we started a transaction for this batch and no explicit START TRANSACTION was seen
        if !was_in_txn && !started_explicit_txn {
            if let Some(txn) = self.current_txn.take() {
                self.transaction_manager.commit(txn);
            }
        }

        Ok(output)
    }

    fn rollback(&mut self, txn: Arc<Mutex<Transaction>>) {
        let mut txn_guard = txn.lock().unwrap();
        // Reverse order for Undo
        let write_set = std::mem::take(&mut txn_guard.write_set);
        drop(txn_guard);

        for record in write_set.into_iter().rev() {
            match record {
                crate::concurrency::transaction::WriteRecord::Insert { table_name, rid } => {
                    if let Some(table_meta) = self.catalog.get_table(&table_name) {
                        // Undo Insert: Get tuple BEFORE deleting to know index keys
                        if let Some(tuple) = table_meta.table.get_tuple(rid) {
                            let indexes = table_meta.indexes.read().unwrap();
                            for (col_idx, index) in indexes.iter() {
                                if let Value::Integer(k) = tuple.get_value(&table_meta.schema, *col_idx) {
                                    index.write().unwrap().delete(k);
                                }
                            }
                        }
                        // Mark as deleted in heap
                        table_meta.table.mark_delete(rid);
                    }
                }
                crate::concurrency::transaction::WriteRecord::Delete { table_name, rid, old_tuple } => {
                    if let Some(table_meta) = self.catalog.get_table(&table_name) {
                        // Undo Delete: Restore in heap
                        table_meta.table.rollback_delete(rid, old_tuple.data.len() as u16);
                        
                        // Undo Delete: Restore in all indexes
                        let indexes = table_meta.indexes.read().unwrap();
                        for (col_idx, index) in indexes.iter() {
                            if let Value::Integer(k) = old_tuple.get_value(&table_meta.schema, *col_idx) {
                                index.write().unwrap().insert(k, rid);
                            }
                        }
                    }
                }
                crate::concurrency::transaction::WriteRecord::Update { table_name, old_rid, old_tuple, new_rid, new_tuple } => {
                    if let Some(table_meta) = self.catalog.get_table(&table_name) {
                        // 1. Undo the new insert in heap
                        table_meta.table.mark_delete(new_rid);
                        
                        // 2. Undo the old delete in heap
                        table_meta.table.rollback_delete(old_rid, old_tuple.data.len() as u16);
                        
                        // 3. Undo Index changes
                        let indexes = table_meta.indexes.read().unwrap();
                        for (col_idx, index) in indexes.iter() {
                             let old_val = old_tuple.get_value(&table_meta.schema, *col_idx);
                             let new_val = new_tuple.get_value(&table_meta.schema, *col_idx);
                             
                             if let (Value::Integer(old_k), Value::Integer(new_k)) = (old_val, new_val) {
                                 let mut idx = index.write().unwrap();
                                 if old_k == new_k {
                                     // Key didn't change, just point back to old_rid
                                     idx.update_value(old_k, old_rid);
                                 } else {
                                     // Key changed, delete the new key and re-insert the old one
                                     idx.delete(new_k);
                                     idx.insert(old_k, old_rid);
                                 }
                             }
                        }
                    }
                }
            }
        }
    }

    fn handle_update(
        &mut self,
        table: sqlparser::ast::TableWithJoins,
        assignments: Vec<sqlparser::ast::Assignment>,
        selection: Option<Expr>,
        txn: Arc<Mutex<Transaction>>,
    ) -> Result<usize, DipError> {
        let table_name = match &table.relation {
            sqlparser::ast::TableFactor::Table { name, .. } => name.to_string(),
            _ => return Err(DipError::Unsupported("Only UPDATE table supported".into())),
        };

        let table_meta = self.catalog.get_table(&table_name)
            .ok_or_else(|| DipError::TableNotFound(format!("Table {} not found", table_name)))?;

        let context = ExecutorContext {
            catalog: table_meta.clone(),
            txn: txn.clone(),
            lock_manager: self.transaction_manager.lock_manager.clone(),
        };

        // Build Child Executor (SeqScan + Filter)
        let mut scan_executor = SeqScanExecutor::new(&context);
        
        // Build Assignments Map
        let mut assign_map = std::collections::HashMap::new();
        for assign in assignments {
            let col_name = match &assign.id[..] {
                [ident] => ident.value.clone(),
                _ => return Err(DipError::Unsupported("Complex column identifiers not supported".into())),
            };
            let col_idx = table_meta.schema.get_col_index(&col_name)
                .ok_or_else(|| DipError::ColumnNotFound(col_name))?;
            let expr = self.parse_expression(assign.value, &table_meta.schema)?;
            assign_map.insert(col_idx, expr);
        }

        let child_exec: Box<dyn Executor>;
        if let Some(expr) = selection {
            let predicate = self.parse_expression(expr, &table_meta.schema)?;
            scan_executor.set_predicate(predicate.clone());
            let scan_exec_box: Box<dyn Executor> = Box::new(scan_executor);
            child_exec = Box::new(FilterExecutor::new(&context, scan_exec_box, predicate));
        } else {
            child_exec = Box::new(scan_executor);
        }

        let mut update_exec = crate::execution::update::UpdateExecutor::new(&context, child_exec, assign_map);
        update_exec.init();

        let mut count = 0;
        while update_exec.next().is_some() {
            count += 1;
        }
        Ok(count)
    }

    fn handle_delete(
        &mut self,
        tables: Vec<ObjectName>,
        selection: Option<Expr>,
        from: Vec<sqlparser::ast::TableWithJoins>,
        txn: Arc<Mutex<Transaction>>,
    ) -> Result<usize, DipError> {
        let name_str = if let Some(table) = from.first() {
            match &table.relation {
                sqlparser::ast::TableFactor::Table { name, .. } => name.to_string(),
                _ => return Err(DipError::Unsupported("Complex DELETE relation not supported".into())),
            }
        } else if let Some(name) = tables.first() {
            name.to_string()
        } else {
             return Err(DipError::Unsupported("No table specified in DELETE".into()));
        };

        let table_meta = self.catalog.get_table(&name_str)
            .ok_or_else(|| DipError::TableNotFound(format!("Table {} not found", name_str)))?;

        let context = ExecutorContext {
            catalog: table_meta.clone(),
            txn: txn.clone(),
            lock_manager: self.transaction_manager.lock_manager.clone(),
        };

        let mut scan_executor = SeqScanExecutor::new(&context);
        let child_exec: Box<dyn Executor>;

        if let Some(expr) = selection {
            let predicate = self.parse_expression(expr, &table_meta.schema)?;
            scan_executor.set_predicate(predicate.clone());
            let scan_exec_box: Box<dyn Executor> = Box::new(scan_executor);
            child_exec = Box::new(FilterExecutor::new(&context, scan_exec_box, predicate));
        } else {
            child_exec = Box::new(scan_executor);
        }

        let mut delete_exec = crate::execution::delete::DeleteExecutor::new(&context, child_exec);
        delete_exec.init();

        let mut count = 0;
        while delete_exec.next().is_some() {
            count += 1;
        }
        Ok(count)
    }

    fn handle_create_index(
        &mut self,
        table_name: ObjectName,
        columns: Vec<sqlparser::ast::OrderByExpr>,
    ) -> Result<(), DipError> {
        let name_str = table_name.to_string();
        let table_meta = self.catalog.get_table(&name_str)
            .ok_or_else(|| DipError::TableNotFound(format!("Table {} not found", name_str)))?;

        let col_name = match &columns.first().ok_or_else(|| DipError::Unsupported("No column specified for index".into()))?.expr {
            Expr::Identifier(ident) => ident.value.clone(),
            _ => return Err(DipError::Unsupported("Only simple column indexes are supported".into())),
        };

        let col_idx = table_meta.schema.get_col_index(&col_name)
            .ok_or_else(|| DipError::ColumnNotFound(col_name))?;

        if table_meta.schema.columns[col_idx].type_id != TypeId::Integer {
            return Err(DipError::Unsupported("Only Integer indexes are supported currently".into()));
        }

        // Create Index
        let btree = Arc::new(RwLock::new(BPlusTree::new(self.catalog.bpm.clone())));
        
        // Populate Index
        let mut iter = table_meta.table.iter();
        while let Some(tuple) = iter.next() {
            if let Some(rid) = tuple.rid {
                 if let Value::Integer(k) = tuple.get_value(&table_meta.schema, col_idx) {
                     btree.write().unwrap().insert(k, rid);
                 }
            }
        }

        // Register in metadata
        table_meta.indexes.write().unwrap().insert(col_idx, btree);

        Ok(())
    }

    fn handle_create_table(&mut self, name: ObjectName, columns: Vec<sqlparser::ast::ColumnDef>) -> Result<(), DipError> {
        let table_name = name.to_string();
        let mut schema_cols = Vec::new();

        for col in columns {
            let col_name = col.name.to_string();
            let type_id = match col.data_type {
                DataType::Int(_) | DataType::Integer(_) => TypeId::Integer,
                DataType::Boolean => TypeId::Boolean,
                DataType::Varchar(_) | DataType::String => TypeId::Varchar,
                _ => return Err(DipError::Unsupported(format!("Data type {:?} not supported", col.data_type))),
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

    fn handle_insert(&mut self, table_name: ObjectName, source: Box<sqlparser::ast::Query>, txn: Arc<Mutex<Transaction>>) -> Result<usize, DipError> {
        let name_str = table_name.to_string();
        let table_meta = self.catalog.get_table(&name_str)
            .ok_or_else(|| DipError::TableNotFound(format!("Table {} not found", name_str)))?;

        // Extract values from the AST
        let rows = match *source.body {
            SetExpr::Values(Values { rows, .. }) => rows,
            _ => return Err(DipError::Unsupported("Only INSERT INTO ... VALUES (...) supported".into())),
        };

        let mut values_batch = Vec::new();

        for row in rows {
            let mut row_values = Vec::new();
            if row.len() != table_meta.schema.column_count() {
                 return Err(DipError::TypeMismatch(format!("Column count mismatch. Expected {}, got {}", table_meta.schema.column_count(), row.len())));
            }

            for (i, expr) in row.iter().enumerate() {
                let target_type = table_meta.schema.columns[i].type_id;
                let val = match (expr, target_type) {
                    (Expr::Value(v), TypeId::Integer) => match v {
                         sqlparser::ast::Value::Number(n, _) => Value::Integer(n.parse().unwrap_or(0)),
                         _ => return Err(DipError::TypeMismatch(format!("Expected Integer at index {}", i))),
                    },
                    (Expr::Value(v), TypeId::Boolean) => match v {
                        sqlparser::ast::Value::Boolean(b) => Value::Boolean(*b),
                        _ => return Err(DipError::TypeMismatch(format!("Expected Boolean at index {}", i))),
                    },
                    (Expr::Value(v), TypeId::Varchar) => match v {
                        sqlparser::ast::Value::SingleQuotedString(s) => Value::Varchar(s.clone()),
                        _ => return Err(DipError::TypeMismatch(format!("Expected String at index {}", i))),
                    },
                    _ => return Err(DipError::TypeMismatch(format!("Type mismatch or unsupported value at index {}", i))),
                };
                row_values.push(val);
            }
            values_batch.push(row_values);
        }

        let count = values_batch.len();
        let context = ExecutorContext { 
            catalog: table_meta.clone(),
            txn: txn.clone(),
            lock_manager: self.transaction_manager.lock_manager.clone(),
        };
        let mut exec = InsertExecutor::new(&context, values_batch);
        exec.init();
        
        // Handle insertion one by one
        // InsertExecutor::next returns Option<Tuple>.
        // BUT, we changed InsertExecutor logic to check PKey.
        // It returns None if PKey duplicate.
        // We need to propagate that error!
        // InsertExecutor needs to return Result<Option<Tuple>, DipError>.
        // But Executor trait is `next(&mut self) -> Option<Tuple>`.
        // This is a limitation.
        // For now, if next() returns None prematurely, we assume it finished OR failed?
        // InsertExecutor returns Some(tuple) for every inserted row.
        // If it returns None, it means no more rows.
        // If we supply 5 rows, and it returns 2 tuples, it failed on 3rd.
        
        let mut success_count = 0;
        while exec.next().is_some() {
            success_count += 1;
        }
        
        if success_count < count {
            return Err(DipError::PkViolation("Duplicate or Lock Error".into()));
        }
        
        Ok(success_count)
    }

    fn handle_query(&mut self, query: Box<sqlparser::ast::Query>, txn: Arc<Mutex<Transaction>>) -> Result<String, DipError> {
        // Assume SELECT * FROM table
        let (table_name, selection) = match query.body.as_ref() {
            SetExpr::Select(select) => {
                 let table_name = match select.from.first() {
                     Some(table_with_joins) => match &table_with_joins.relation {
                         sqlparser::ast::TableFactor::Table { name, .. } => name.to_string(),
                         _ => return Err(DipError::Unsupported("Only SELECT FROM table supported".into())),
                     },
                     None => return Err(DipError::Unsupported("No table specified".into())),
                 };
                 (table_name, select.selection.clone())
            },
            _ => return Err(DipError::Unsupported("Only SELECT queries supported".into())),
        };

        let table_meta = self.catalog.get_table(&table_name)
            .ok_or_else(|| DipError::TableNotFound(format!("Table {} not found", table_name)))?;
        
        let context = ExecutorContext { 
            catalog: table_meta.clone(),
            txn: txn.clone(),
            lock_manager: self.transaction_manager.lock_manager.clone(),
        };
        
        // Build Executor Chain
        let mut root_exec: Box<dyn Executor>;

        if let Some(expr) = selection {
            let predicate = self.parse_expression(expr, &table_meta.schema)?;
            
            // Optimization: Check for Index Scan
            let mut use_index = false;
            let mut key_val = 0;
            let mut best_col_idx = 0;

            let indexes = table_meta.indexes.read().unwrap();
            if !indexes.is_empty() {
                 if let Expression::Binary { left, op, right } = &predicate {
                     if *op == BinaryOperator::Eq {
                         if let (Expression::Column(idx), Expression::Constant(val)) = (left.as_ref(), right.as_ref()) {
                             if indexes.contains_key(idx) {
                                 if let Value::Integer(k) = val {
                                     use_index = true;
                                     key_val = *k;
                                     best_col_idx = *idx;
                                 }
                             }
                         }
                     }
                 }
            }
            drop(indexes);

            if use_index {
                root_exec = Box::new(IndexScanExecutor::new(&context, best_col_idx, key_val));
            } else {
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

    fn parse_expression(&self, expr: Expr, schema: &Schema) -> Result<Expression, DipError> {
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
                    _ => return Err(DipError::Unsupported(format!("Binary Operator {:?} not supported", op))),
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
                    .ok_or_else(|| DipError::ColumnNotFound(format!("Column {} not found", name)))?;
                Ok(Expression::Column(idx))
            }
            Expr::Value(v) => {
                 match v {
                     sqlparser::ast::Value::Number(n, _) => Ok(Expression::Constant(Value::Integer(n.parse().unwrap_or(0)))),
                     sqlparser::ast::Value::Boolean(b) => Ok(Expression::Constant(Value::Boolean(b))),
                     sqlparser::ast::Value::SingleQuotedString(s) => Ok(Expression::Constant(Value::Varchar(s))),
                     _ => Err(DipError::Unsupported(format!("Value {:?} not supported in expression", v))),
                 }
            }
            _ => Err(DipError::Unsupported(format!("Expression {:?} not supported", expr))),
        }
    }
}
