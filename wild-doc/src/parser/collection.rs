use std::sync::Arc;

use hashbrown::HashMap;

use wild_doc_script::WildDocValue;

use super::{AttributeMap, Parser};

impl Parser {
    pub(super) fn collections(&self, attributes: AttributeMap) {
        let mut vars = HashMap::new();

        if let Some(Some(var)) = attributes.get("var") {
            let var = var.to_str();
            if var != "" {
                vars.insert(
                    var.into(),
                    Arc::new(WildDocValue::Array(
                        self.database
                            .read()
                            .collections()
                            .into_iter()
                            .map(|v| Arc::new(WildDocValue::String(v)))
                            .collect(),
                    )),
                );
            }
        }
        self.state.stack().lock().push(vars);
    }

    pub(super) async fn delete_collection(&self, attributes: AttributeMap) {
        if let Some(Some(collection)) = attributes.get("collection") {
            self.database
                .write()
                .delete_collection(&collection.to_str())
                .await;
        }
    }
}
