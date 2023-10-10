use std::{ops::Deref, sync::Arc};

use anyhow::Result;
use parking_lot::{Mutex, RwLock};

use wild_doc_script::{VarsStack, WildDocScript, WildDocValue};

pub struct Var {
    stack: Arc<Mutex<VarsStack>>,
}

impl Var {
    fn search_stack(&self, key: &[u8]) -> Option<Arc<RwLock<WildDocValue>>> {
        for stack in self.stack.lock().iter().rev() {
            if let Some(v) = stack.get(key) {
                return Some(Arc::clone(v));
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
            stack: Arc::clone(state.stack()),
        })
    }

    fn evaluate_module(&self, _: &str, _: &[u8]) -> Result<()> {
        Ok(())
    }

    fn eval(&self, code: &[u8]) -> Result<WildDocValue> {
        let mut value = WildDocValue::Null;

        let mut splited = code.split(|c| *c == b'.');
        if let Some(root) = splited.next() {
            if let Some(root) = self.search_stack(root) {
                let next_value = root.read().deref().clone();
                let mut next_value = &next_value;
                while {
                    splited.next().map_or_else(
                        || {
                            value = next_value.clone();
                            false
                        },
                        |next| match next_value {
                            WildDocValue::Object(map) => map
                                .get(unsafe { std::str::from_utf8_unchecked(next) })
                                .map_or(false, |v| {
                                    next_value = v;
                                    true
                                }),
                            WildDocValue::Array(map) => {
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
