use std::sync::Arc;

use wild_doc_script::{Vars, WildDocValue};

use super::Parser;

impl Parser {
    #[must_use]
    pub(super) fn collections(&self, vars: Vars) ->Vars{
        let mut r = Vars::new();

        if let Some(var) = vars.get("var") {
            let var = var.to_str();
            if var != "" {
                r.insert(
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
        r
    }

    pub(super) async fn delete_collection(&self, vars: Vars) {
        if let Some(collection) = vars.get("collection") {
            self.database
                .write()
                .delete_collection(&collection.to_str())
                .await;
        }
    }
}
