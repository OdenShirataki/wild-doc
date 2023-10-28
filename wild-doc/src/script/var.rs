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
        Ok(Self { state })
    }

    async fn evaluate_module(&mut self, _: &str, _: &[u8]) -> Result<()> {
        Ok(())
    }

    async fn eval(&mut self, code: &[u8]) -> Result<Arc<WildDocValue>> {
        let mut splited = code.split(|c| *c == b'.');
        if let Some(root) = splited.next() {
            if let Some(root) = if root == b"global" {
                if let Some(global_root) = splited.next() {
                    self.state
                        .global()
                        .lock()
                        .get(unsafe { std::str::from_utf8_unchecked(global_root) })
                        .cloned()
                } else {
                    None
                }
            } else {
                self.search_stack(root)
            } {
                if let Some(next) = splited.next() {
                    if let Some(mut next_value) = match root.as_ref() {
                        WildDocValue::Object(map) => {
                            map.get(unsafe { std::str::from_utf8_unchecked(next) })
                        }
                        WildDocValue::Array(map) => unsafe { std::str::from_utf8_unchecked(next) }
                            .parse::<usize>()
                            .map_or(None, |v| map.get(v)),
                        _ => None,
                    } {
                        loop {
                            if let Some(next) = splited.next() {
                                match next_value.as_ref() {
                                    WildDocValue::Object(map) => {
                                        if let Some(v) =
                                            map.get(unsafe { std::str::from_utf8_unchecked(next) })
                                        {
                                            next_value = v;
                                        }
                                    }
                                    WildDocValue::Array(map) => {
                                        if let Ok(v) =
                                            unsafe { std::str::from_utf8_unchecked(next) }
                                                .parse::<usize>()
                                        {
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
                } else {
                    return Ok(Arc::clone(&root));
                }
            }
        }

        Ok(Arc::new(WildDocValue::Null))
    }
}
