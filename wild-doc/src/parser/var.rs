use std::{collections::VecDeque, sync::Arc};

use indexmap::IndexMap;
use wild_doc_script::WildDocValue;

use super::{AttributeMap, Parser};

impl Parser {
    fn route_map<'a>(
        map: &mut IndexMap<String, Arc<WildDocValue>>,
        mut keys: VecDeque<&str>,
        last_val: &Arc<WildDocValue>,
    ) -> Option<Arc<WildDocValue>> {
        keys.pop_front().and_then(|key| {
            if keys.is_empty() {
                map.insert(key.to_string(), Arc::clone(last_val));
            } else {
                if let Some(rm) = Self::route_map(
                    &mut if let Some(WildDocValue::Object(o)) = map.get(key).map(|v| v.as_ref()) {
                        o.clone()
                    } else {
                        IndexMap::new()
                    },
                    keys,
                    last_val,
                ) {
                    map.insert(key.to_string(), Arc::clone(&rm));
                    return Some(rm);
                }
            }
            None
        })
    }

    #[inline(always)]
    pub(crate) fn register_global(&self, name: &str, value: &Arc<WildDocValue>) {
        let mut splited: VecDeque<_> = name.split('.').collect();
        if let Some(last) = splited.pop_back() {
            if splited.len() > 0 {
                Self::route_map(&mut self.state.global().lock(), splited, value);
            } else {
                self.state
                    .global()
                    .lock()
                    .insert(last.to_owned(), Arc::clone(value));
            }
        }
    }

    #[inline(always)]
    pub(super) fn local(&self, attributes: AttributeMap) {
        self.state.stack().lock().push(
            attributes
                .into_iter()
                .map(|(k, v)| (k, v.unwrap_or_else(|| Arc::new(WildDocValue::Null))))
                .collect(),
        );
    }
}
