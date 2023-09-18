use std::{
    collections::VecDeque,
    sync::{Arc, RwLock},
};

use bson::{Bson, Document};

use super::{AttributeMap, Parser};

impl Parser {
    fn route_doc<'a, 'b>(doc: &mut Document, mut keys: VecDeque<&str>) -> Option<&'a mut Document> {
        if let Some(key) = keys.pop_front() {
            if let Ok(next) = doc.get_document_mut(key) {
                return Self::route_doc(next, keys);
            } else {
                doc.insert(key, Document::new());
                if let Ok(next) = doc.get_document_mut(key) {
                    return Self::route_doc(next, keys);
                }
            }
        }
        None
    }

    #[inline(always)]
    pub(crate) fn register_global(&mut self, name: &str, value: &Bson) {
        if let Some(stack) = self.state.stack().write().unwrap().get(0) {
            if let Some(global) = stack.get(b"global".as_ref()) {
                if let Ok(mut global) = global.write() {
                    if let Some(doc) = global.as_document_mut() {
                        let mut splited: VecDeque<_> = name.split('.').collect();
                        if let Some(last) = splited.pop_back() {
                            if splited.len() > 0 {
                                if let Some(doc) = Self::route_doc(doc, splited) {
                                    doc.insert(last, value);
                                }
                            } else {
                                doc.insert(last, value);
                            }
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
