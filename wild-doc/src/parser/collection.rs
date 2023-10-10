use std::sync::Arc;

use hashbrown::HashMap;

use parking_lot::RwLock;
use wild_doc_script::WildDocValue;

use super::{AttributeMap, Parser};

impl Parser {
    pub(super) fn collections(&self, attributes: AttributeMap) {
        let mut vars = HashMap::new();

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            let var = var.to_str();
            if var != "" {
                vars.insert(
                    var.to_string().into_bytes(),
                    Arc::new(RwLock::new(WildDocValue::Array(
                        self.database
                            .read()
                            .collections()
                            .iter()
                            .map(|v| WildDocValue::String(v.to_owned()))
                            .collect(),
                    ))),
                );
            }
        }
        self.state.stack().lock().push(vars);
    }

    pub(super) fn delete_collection(&self, attributes: AttributeMap) {
        if let Some(Some(collection)) = attributes.get(b"collection".as_ref()) {
            futures::executor::block_on(
                self.database
                    .write()
                    .delete_collection(collection.to_str().as_ref()),
            );
        }
    }
}
