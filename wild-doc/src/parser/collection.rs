use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use bson::Bson;

use super::{AttributeMap, Parser};

impl Parser {
    pub(super) fn collections(&mut self, attributes: AttributeMap) {
        let mut bson = HashMap::new();

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            if let Some(var) = var.as_str() {
                if var != "" {
                    bson.insert(
                        var.as_bytes().to_vec(),
                        Arc::new(RwLock::new(Bson::Array(
                            self.database
                                .read()
                                .unwrap()
                                .collections()
                                .iter()
                                .map(|v| Bson::String(v.clone()))
                                .collect(),
                        ))),
                    );
                }
            }
        }
        self.state.stack().write().unwrap().push(bson);
    }

    pub(super) fn delete_collection(&mut self, attributes: AttributeMap) {
        if let Some(Some(collection)) = attributes.get(b"collection".as_ref()) {
            self.database
                .clone()
                .write()
                .unwrap()
                .delete_collection(&collection.to_string());
        }
    }
}
