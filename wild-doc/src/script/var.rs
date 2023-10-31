use std::sync::Arc;

use anyhow::Result;

use wild_doc_script::{async_trait, Vars, WildDocScript, WildDocState, WildDocValue};

pub struct Var {}

#[async_trait(?Send)]
impl WildDocScript for Var {
    fn new(_: Arc<WildDocState>) -> Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {})
    }

    async fn evaluate_module(&mut self, _: &str, _: &str, _: &Vars) -> Result<()> {
        Ok(())
    }

    async fn eval(&mut self, code: &str, stack: &Vars) -> Result<Arc<WildDocValue>> {
        let mut splited = code.split(".");
        if let Some(root) = splited.next() {
            if let Some(mut next_value) = stack.get(root) {
                loop {
                    if let Some(next) = splited.next() {
                        match next_value.as_ref() {
                            WildDocValue::Object(map) => {
                                if let Some(v) = map.get(next) {
                                    next_value = v;
                                }
                            }
                            WildDocValue::Array(map) => {
                                if let Ok(v) = next.parse::<usize>() {
                                    if let Some(v) = map.get(v) {
                                        next_value = v;
                                    }
                                }
                            }
                            _ => break,
                        }
                    } else {
                        return Ok(Arc::clone(next_value));
                    }
                }
            }
        }

        Ok(Arc::new(WildDocValue::Null))
    }
}
