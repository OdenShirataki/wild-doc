use std::sync::Arc;

use anyhow::Result;

use wild_doc_script::{async_trait, WildDocScript, WildDocState, WildDocValue};

pub struct Var {
    state: Arc<WildDocState>,
}

impl Var {
    fn search_stack(&self, key: &[u8]) -> Option<Arc<WildDocValue>> {
        for stack in self.state.stack().lock().iter().rev() {
            if let Some(v) = stack.get(key) {
                return Some(Arc::clone(v));
            }
        }
        None
    }
}

#[async_trait(?Send)]
impl WildDocScript for Var {
    fn new(state: Arc<WildDocState>) -> Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            state: Arc::clone(&state),
        })
    }

    async fn evaluate_module(&self, _: &str, _: &[u8]) -> Result<()> {
        Ok(())
    }

    async fn eval(&self, code: &[u8]) -> Result<WildDocValue> {
        let mut value = WildDocValue::Null;

        let mut splited = code.split(|c| *c == b'.');
        if let Some(root) = splited.next() {
            if root == b"global" {
                if let Some(global_root) = splited.next() {
                    if let Some(next_value) = self
                        .state
                        .global()
                        .lock()
                        .get(unsafe { std::str::from_utf8_unchecked(global_root) })
                    {
                        let mut next_value = next_value;
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
                                            .map_or(false, |v| {
                                                map.get(v).map_or(false, |v| {
                                                    next_value = v;
                                                    true
                                                })
                                            })
                                    }
                                    _ => false,
                                },
                            )
                        } {}
                    }
                }
            } else {
                if let Some(ref root) = self.search_stack(root) {
                    let mut next_value = root.as_ref();
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
                                        .map_or(false, |v| {
                                            map.get(v).map_or(false, |v| {
                                                next_value = v;
                                                true
                                            })
                                        })
                                }
                                _ => false,
                            },
                        )
                    } {}
                }
            }
        }

        Ok(value)
    }
}
