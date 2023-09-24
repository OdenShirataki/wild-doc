use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use wild_doc_script::WildDocValue;

use super::{AttributeMap, Parser};

impl Parser {
    pub(super) fn collections(&mut self, attributes: AttributeMap) {
        let mut vars = HashMap::new();

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            let var = var.to_str();
            if var != "" {
                vars.insert(
                    var.to_string().into_bytes(),
                    Arc::new(RwLock::new(WildDocValue::Array(
                        self.database
                            .read()
                            .unwrap()
                            .collections()
                            .iter()
                            .map(|v| WildDocValue::String(v.to_owned()))
                            .collect(),
                    ))),
                );
            }
        }
        self.state.stack().write().unwrap().push(vars);
    }

    pub(super) fn delete_collection(&mut self, attributes: AttributeMap) {
        if let Some(Some(collection)) = attributes.get(b"collection".as_ref()) {
            self.database
                .clone()
                .write()
                .unwrap()
                .delete_collection(collection.to_str().as_ref());
        }
    }
}
