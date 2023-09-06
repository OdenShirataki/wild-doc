use std::sync::{Arc, RwLock};

use serde_json::Value;

use super::{AttributeMap, Parser};

impl Parser {
    pub(crate) fn register_global(&mut self, name: &str, value: &serde_json::Value) {
        if let Some(stack) = self.state.stack().write().unwrap().get(0) {
            if let Some(global) = stack.get(b"global".as_ref()) {
                if let Ok(mut global) = global.write() {
                    let mut json: &mut Value = &mut global;
                    let splited = name.split('.');
                    for s in splited {
                        if !json[s].is_object() {
                            json[s] = serde_json::json!({});
                        }
                        json = &mut json[s];
                    }
                    *json = value.clone();
                }
            }
        }
    }

    pub(super) fn local(&mut self, attributes: AttributeMap) {
        self.state.stack().write().unwrap().push(
            attributes
                .iter()
                .filter_map(|(k, v)| {
                    v.as_ref()
                        .map(|v| (k.to_vec(), Arc::new(RwLock::new(v.as_ref().clone()))))
                })
                .collect(),
        );
    }
}
