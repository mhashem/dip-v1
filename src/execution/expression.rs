use crate::types::{Value, TypeId};
use crate::storage::tuple::Tuple;
use crate::catalog::schema::Schema;

#[derive(Debug, Clone)]
pub enum Expression {
    /// A constant value (e.g., 5, "Alice")
    Constant(Value),
    /// A reference to a column by index (e.g., column 0)
    Column(usize),
    /// A binary operation (e.g., A = B, A > B)
    Binary {
        left: Box<Expression>,
        op: BinaryOperator,
        right: Box<Expression>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
}

impl Expression {
    pub fn evaluate(&self, tuple: &Tuple, schema: &Schema) -> Value {
        match self {
            Expression::Constant(v) => v.clone(),
            Expression::Column(idx) => tuple.get_value(schema, *idx),
            Expression::Binary { left, op, right } => {
                let l_val = left.evaluate(tuple, schema);
                let r_val = right.evaluate(tuple, schema);
                
                let result = match op {
                    BinaryOperator::Eq => l_val == r_val,
                    BinaryOperator::NotEq => l_val != r_val,
                    BinaryOperator::Lt => l_val < r_val,
                    BinaryOperator::Gt => l_val > r_val,
                    BinaryOperator::LtEq => l_val <= r_val,
                    BinaryOperator::GtEq => l_val >= r_val,
                };
                
                Value::Boolean(result)
            }
        }
    }
}
