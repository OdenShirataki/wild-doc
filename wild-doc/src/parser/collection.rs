use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use wild_doc_script::WildDocValue;

use super::{AttributeMap, Parser};

impl Parser {
    pub(super) fn collections(&mut self, attributes: AttributeMap) {
        let mut json = HashMap::new();

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            let var = var.to_str();
            if var != "" {
                let collections = self.database.read().unwrap().collections();
                json.insert(
                    var.to_string().as_bytes().to_vec(),
                    Arc::new(RwLock::new(WildDocValue::new(serde_json::json!(
                        collections
                    )))),
                );
            }
        }
        self.state.stack().write().unwrap().push(json);
    }

    pub(super) fn delete_collection(&mut self, attributes: AttributeMap) {
        if let Some(Some(str_collection)) = attributes.get(b"collection".as_ref()) {
            self.database
                .clone()
                .write()
                .unwrap()
                .delete_collection(str_collection.to_str().as_ref());
        }
    }
}
