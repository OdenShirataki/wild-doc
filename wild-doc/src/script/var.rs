use std::sync::Arc;

use anyhow::Result;

use wild_doc_script::{async_trait, WildDocScript, WildDocState, WildDocValue};

pub struct Var {
    state: Arc<WildDocState>,
}

impl Var {
    fn search_stack(&self, key: &str) -> Option<Arc<WildDocValue>> {
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
            if let Some(mut next_value) = self
                .search_stack(unsafe { std::str::from_utf8_unchecked(root) })
                .as_ref()
            {
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
                                    unsafe { std::str::from_utf8_unchecked(next) }.parse::<usize>()
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
        }

        Ok(Arc::new(WildDocValue::Null))
    }
}
