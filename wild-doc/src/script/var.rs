use std::{path::PathBuf, sync::Arc};

use anyhow::Result;

use parking_lot::Mutex;
use wild_doc_script::{async_trait, IncludeAdaptor, Stack, WildDocScript, WildDocValue};

pub struct Var {}

#[async_trait(?Send)]
impl<I: IncludeAdaptor + Send> WildDocScript<I> for Var {
    fn new(_: Arc<Mutex<I>>, _: PathBuf, _: &Stack) -> Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {})
    }

    async fn evaluate_module(&mut self, _: &str, _: &str, _: &Stack) -> Result<()> {
        Ok(())
    }

    async fn eval(&mut self, code: &str, stack: &Stack) -> Result<WildDocValue> {
        let mut splited = code.split(".");
        if let Some(root) = splited.next() {
            if let Some(mut next_value) = stack.get(&Arc::new(root.into())) {
                loop {
                    if let Some(next) = splited.next() {
                        match next_value {
                            WildDocValue::Object(map) => {
                                if let Some(v) = map.get(&Arc::new(next.into())) {
                                    next_value = v;
                                } else {
                                    break;
                                }
                            }
                            WildDocValue::Array(map) => {
                                let mut found = false;
                                if let Ok(v) = next.parse::<usize>() {
                                    if let Some(v) = map.get(v) {
                                        found = true;
                                        next_value = v;
                                    }
                                }
                                if !found {
                                    break;
                                }
                            }
                            WildDocValue::SearchResult(result) => {
                                if next == "rows" {
                                    if let Some(next) = splited.next() {
                                        if next == "len" {
                                            return Ok(WildDocValue::Number(
                                                result.rows().len().into(),
                                            ));
                                        }
                                    } else {
                                        return Ok(WildDocValue::Array(
                                            result
                                                .rows()
                                                .into_iter()
                                                .map(|v| WildDocValue::Number(v.get().into()))
                                                .collect(),
                                        ));
                                    }
                                }
                            }
                            WildDocValue::SessionSearchResult(result) => {
                                if next == "rows" {
                                    if let Some(next) = splited.next() {
                                        if next == "len" {
                                            return Ok(WildDocValue::Number(
                                                result.rows().len().into(),
                                            ));
                                        }
                                    } else {
                                        return Ok(WildDocValue::Array(
                                            result
                                                .rows()
                                                .into_iter()
                                                .map(|v| WildDocValue::Number(v.get().into()))
                                                .collect(),
                                        ));
                                    }
                                }
                            }
                            _ => break,
                        }
                    } else {
                        return Ok(next_value.clone());
                    }
                }
            }
        }

        Ok(WildDocValue::Null)
    }
}
