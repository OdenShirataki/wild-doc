use std::sync::{Arc, RwLock};

use anyhow::Result;

use wild_doc_script::{VarsStack, WildDocScript, WildDocValue};

pub struct Var {
    stack: Arc<RwLock<VarsStack>>,
}
impl Var {
    fn search_stack(&self, key: &[u8]) -> Option<Arc<RwLock<WildDocValue>>> {
        for stack in self.stack.read().unwrap().iter().rev() {
            if let Some(v) = stack.get(key) {
                return Some(v.clone());
            }
        }
        None
    }
}
impl WildDocScript for Var {
    fn new(state: wild_doc_script::WildDocState) -> Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            stack: state.stack(),
        })
    }

    fn evaluate_module(&mut self, _: &str, _: &[u8]) -> Result<()> {
        Ok(())
    }

    fn eval(&mut self, code: &[u8]) -> Result<serde_json::Value> {
        let mut value = serde_json::json!("");

        let mut splited = code.split(|c| *c == b'.');
        if let Some(root) = splited.next() {
            if let Some(root) = self.search_stack(root) {
                let next_value = root.read().unwrap();
                let mut next_value = next_value.value();
                while {
                    splited.next().map_or_else(
                        || {
                            value = next_value.clone();
                            false
                        },
                        |next| match next_value {
                            serde_json::Value::Object(map) => map
                                .get(unsafe { std::str::from_utf8_unchecked(next) })
                                .map_or(false, |v| {
                                    next_value = v;
                                    true
                                }),
                            serde_json::Value::Array(map) => {
                                unsafe { std::str::from_utf8_unchecked(next) }
                                    .parse::<usize>()
                                    .ok()
                                    .and_then(|v| map.get(v))
                                    .map_or(false, |v| {
                                        next_value = v;
                                        true
                                    })
                            }
                            _ => false,
                        },
                    )
                } {}
            }
        }
        Ok(value)
    }
}
