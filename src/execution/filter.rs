use crate::execution::executor::{Executor, ExecutorContext};
use crate::storage::tuple::Tuple;
use crate::catalog::schema::Schema;
use crate::execution::expression::Expression;
use crate::types::Value;

pub struct FilterExecutor<'a> {
    context: &'a ExecutorContext,
    child: Box<dyn Executor + 'a>,
    predicate: Expression,
}

impl<'a> FilterExecutor<'a> {
    pub fn new(context: &'a ExecutorContext, child: Box<dyn Executor + 'a>, predicate: Expression) -> Self {
        Self {
            context,
            child,
            predicate,
        }
    }
}

impl<'a> Executor for FilterExecutor<'a> {
    fn init(&mut self) {
        self.child.init();
    }

    fn next(&mut self) -> Option<Tuple> {
        while let Some(tuple) = self.child.next() {
            let result = self.predicate.evaluate(&tuple, self.child.schema());
            
            if let Value::Boolean(true) = result {
                return Some(tuple);
            }
        }
        None
    }

    fn schema(&self) -> &Schema {
        self.child.schema()
    }
}
