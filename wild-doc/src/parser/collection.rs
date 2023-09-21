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
            let var = var.to_str();
            if var != "" {
                let collections = self.database.read().unwrap().collections();
                json.insert(
                    var.to_string().into_bytes(),
                    Arc::new(RwLock::new(WildDocValue::from(
                        serde_json::json!(collections).to_string().into_bytes(),
                    ))),
                );
            }
        }
        self.state.stack().write().unwrap().push(json);
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
