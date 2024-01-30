use std::sync::Arc;

use wild_doc_script::{Vars, WildDocValue};

use super::Parser;

impl Parser {
    #[must_use]
    pub(super) fn collections(&self, vars: Vars) -> Vars {
        let mut r = Vars::new();

        if let Some(var) = vars.get(&self.strings.var) {
            let var = var.as_string();
            if var.as_str() != "" {
                r.insert(
                    var,
                    WildDocValue::Array(
                        self.database
                            .read()
                            .collections()
                            .into_iter()
                            .map(|v| WildDocValue::String(Arc::new(v)))
                            .collect(),
                    ),
                );
            }
        }
        r
    }

    pub(super) async fn delete_collection(&self, vars: Vars) {
        if let Some(collection) = vars.get(&self.strings.collection) {
            self.database
                .write()
                .delete_collection(&collection.as_string())
                .await;
        }
    }
}
