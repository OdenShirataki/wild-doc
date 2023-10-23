use std::{collections::VecDeque, sync::Arc};

use indexmap::IndexMap;
use wild_doc_script::WildDocValue;

use super::{AttributeMap, Parser};

impl Parser {
    fn route_map<'a>(
        map: &mut IndexMap<String, WildDocValue>,
        mut keys: VecDeque<&str>,
    ) -> Option<&'a mut IndexMap<String, WildDocValue>> {
        keys.pop_front().and_then(|key| match map.get_mut(key) {
            Some(WildDocValue::Object(next)) => Self::route_map(next, keys),
            _ => {
                let mut next = IndexMap::new();
                let r = Self::route_map(&mut next, keys);
                map.insert(key.to_string(), WildDocValue::Object(next));
                r
            }
        })
    }

    #[inline(always)]
    pub(crate) fn register_global(&self, name: &str, value: WildDocValue) {
        let mut splited: VecDeque<_> = name.split('.').collect();
        if let Some(last) = splited.pop_back() {
            if splited.len() > 0 {
                if let Some(map) = Self::route_map(&mut self.state.global().lock(), splited) {
                    map.insert(last.to_owned(), value);
                }
            } else {
                self.state.global().lock().insert(last.to_owned(), value);
            }
        }
    }

    #[inline(always)]
    pub(super) fn local(&self, attributes: AttributeMap) {
        self.state.stack().lock().push(
            attributes
                .into_iter()
                .map(|(k, v)| (k, Arc::new(v.unwrap_or(WildDocValue::Null))))
                .collect(),
        );
    }
}
