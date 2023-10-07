use std::{
    collections::VecDeque,
    ops::DerefMut,
    sync::{Arc, RwLock},
};

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
    pub(crate) fn register_global(&self, name: &str, value: &WildDocValue) {
        if let Some(global) = self
            .state
            .stack()
            .write()
            .unwrap()
            .get(0)
            .and_then(|v| v.get(b"global".as_ref()))
        {
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

    #[inline(always)]
    pub(super) fn local(&self, attributes: AttributeMap) {
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
