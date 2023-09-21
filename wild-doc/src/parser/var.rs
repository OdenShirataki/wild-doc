use std::{
    collections::VecDeque,
    ops::DerefMut,
    sync::{Arc, RwLock},
};

use indexmap::IndexMap;

use wild_doc_script::WildDocValue;

use super::{AttributeMap, Parser};

impl Parser {
    fn route_map<'a, 'b>(
        map: &mut IndexMap<String, WildDocValue>,
        mut keys: VecDeque<&str>,
    ) -> Option<&'a mut IndexMap<String, WildDocValue>> {
        if let Some(key) = keys.pop_front() {
            if let Some(WildDocValue::Object(next)) = map.get_mut(key) {
                return Self::route_map(next, keys);
            } else {
                let mut next = IndexMap::new();
                let r = Self::route_map(&mut next, keys);
                map.insert(key.to_string(), WildDocValue::Object(next));
                return r;
            }
        }
        None
    }

    pub(crate) fn register_global(&mut self, name: &str, value: &WildDocValue) {
        if let Some(stack) = self.state.stack().write().unwrap().get(0) {
            if let Some(global) = stack.get(b"global".as_ref()) {
                if let WildDocValue::Object(ref mut map) = global.write().unwrap().deref_mut() {
                    let mut splited: VecDeque<_> = name.split('.').collect();
                    if let Some(last) = splited.pop_back() {
                        if splited.len() > 0 {
                            if let Some(map) = Self::route_map(map, splited) {
                                map.insert(last.to_owned(), value.clone());
                            }
                        } else {
                            map.insert(last.to_owned(), value.clone());
                        }
                    }
                }
            }
        }
    }

    #[inline(always)]
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
