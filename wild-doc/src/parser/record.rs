use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use indexmap::IndexMap;

use semilattice_database_session::{Activity, CollectionRow, Uuid};
use wild_doc_script::WildDocValue;

use super::Parser;

impl Parser {
    pub(super) fn record(&mut self, attributes: HashMap<Vec<u8>, Option<Arc<WildDocValue>>>) {
        let mut obj = HashMap::new();

        if let (Some(Some(collection)), Some(Some(row)), Some(Some(var))) = (
            attributes.get(b"collection".as_ref()),
            attributes.get(b"row".as_ref()),
            attributes.get(b"var".as_ref()),
        ) {
            let var = var.to_str();
            if var != "" {
                let database = self.database.read().unwrap();
                let mut map = IndexMap::new();
                if let Some(collection_id) = database.collection_id(&collection.to_string()) {
                    let mut session_maybe_has_collection = None;
                    for i in (0..self.sessions.len()).rev() {
                        if let Some(temporary_collection) =
                            self.sessions[i].session.temporary_collection(collection_id)
                        {
                            session_maybe_has_collection = Some(temporary_collection);
                            break;
                        }
                    }

                    if let Ok(row) = row.to_string().parse::<i64>() {
                        map.insert(
                            "row".to_owned(),
                            WildDocValue::from(serde_json::Number::from(row)),
                        );
                        if let Some(temporary_collection) = session_maybe_has_collection {
                            if let Some(entity) = temporary_collection.get(&row) {
                                map.insert(
                                    "uuid".to_owned(),
                                    WildDocValue::from(Uuid::from_u128(entity.uuid()).to_string()),
                                );
                                map.insert(
                                    "activity".to_owned(),
                                    WildDocValue::from(entity.activity() == Activity::Active),
                                );
                                map.insert(
                                    "term_begin".to_owned(),
                                    WildDocValue::from(serde_json::Number::from(
                                        entity.term_begin(),
                                    )),
                                );
                                map.insert(
                                    "term_begin".to_owned(),
                                    WildDocValue::from(serde_json::Number::from(entity.term_end())),
                                );

                                let mut depends = IndexMap::new();
                                for d in entity.depends() {
                                    let mut depend = IndexMap::new();
                                    depend.insert(
                                        "collection_id".to_owned(),
                                        WildDocValue::from(serde_json::Number::from(
                                            d.collection_id(),
                                        )),
                                    );
                                    depend.insert(
                                        "row".to_owned(),
                                        WildDocValue::from(serde_json::Number::from(d.row())),
                                    );

                                    depends.insert(d.key().to_string(), WildDocValue::from(depend));
                                }
                                map.insert("depends".to_owned(), WildDocValue::from(depends));

                                let mut field = IndexMap::new();
                                for (field_name, value) in entity.fields() {
                                    field.insert(
                                        field_name.as_str().to_owned(),
                                        if let Ok(str) = std::str::from_utf8(value) {
                                            WildDocValue::from(str.to_owned())
                                        } else {
                                            WildDocValue::from(value.to_owned())
                                        },
                                    );
                                }
                                map.insert("field".to_owned(), WildDocValue::from(field));
                            }
                        } else {
                            if row > 0 {
                                if let Some(collection) =
                                    self.database.read().unwrap().collection(collection_id)
                                {
                                    let row = row as u32;

                                    if let Some(uuid) = collection.uuid_string(row) {
                                        map.insert("uuid".to_owned(), WildDocValue::from(uuid));
                                    }
                                    if let Some(activity) = collection.activity(row) {
                                        map.insert(
                                            "activity".to_owned(),
                                            WildDocValue::from(activity == Activity::Active),
                                        );
                                    };
                                    if let Some(term_begin) = collection.term_begin(row) {
                                        map.insert(
                                            "term_begin".to_owned(),
                                            WildDocValue::from(serde_json::Number::from(
                                                term_begin,
                                            )),
                                        );
                                    }
                                    if let Some(term_end) = collection.term_end(row) {
                                        map.insert(
                                            "term_end".to_owned(),
                                            WildDocValue::from(serde_json::Number::from(term_end)),
                                        );
                                    }
                                    if let Some(last_updated) = collection.last_updated(row) {
                                        map.insert(
                                            "last_updated".to_owned(),
                                            WildDocValue::from(serde_json::Number::from(
                                                last_updated,
                                            )),
                                        );
                                    }
                                    let mut depends = IndexMap::new();
                                    for d in self
                                        .database
                                        .read()
                                        .unwrap()
                                        .relation()
                                        .read()
                                        .unwrap()
                                        .depends(None, &CollectionRow::new(collection_id, row))
                                    {
                                        let mut depend = IndexMap::new();

                                        let collection_id = d.collection_id();

                                        if let Some(collection) =
                                            self.database.read().unwrap().collection(collection_id)
                                        {
                                            depend.insert(
                                                "collection_id".to_owned(),
                                                WildDocValue::from(serde_json::Number::from(
                                                    collection_id,
                                                )),
                                            );
                                            depend.insert(
                                                "collection_name".to_owned(),
                                                WildDocValue::from(collection.name().to_owned()),
                                            );
                                            depend.insert(
                                                "row".to_owned(),
                                                WildDocValue::from(serde_json::Number::from(
                                                    d.row(),
                                                )),
                                            );
                                            depends.insert(
                                                d.key().to_string(),
                                                WildDocValue::from(depend),
                                            );
                                        }
                                    }
                                    map.insert("depends".to_owned(), WildDocValue::from(depends));

                                    let mut field = IndexMap::new();
                                    for field_name in collection.field_names() {
                                        let bytes = collection.field_bytes(row, field_name);
                                        field.insert(
                                            field_name.as_str().to_owned(),
                                            if let Ok(str) = std::str::from_utf8(bytes) {
                                                WildDocValue::from(str.to_owned())
                                            } else {
                                                WildDocValue::from(bytes.to_owned())
                                            },
                                        );
                                    }
                                    map.insert("field".to_owned(), WildDocValue::from(field));
                                }
                            }
                        }
                    }
                }
                obj.insert(
                    var.to_string().into_bytes(),
                    Arc::new(RwLock::new(WildDocValue::from(map))),
                );
            }
        }
        self.state.stack().write().unwrap().push(obj);
    }
}
