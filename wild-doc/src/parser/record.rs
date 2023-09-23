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
        let mut json = HashMap::new();

        if let (Some(Some(collection)), Some(Some(row)), Some(Some(var))) = (
            attributes.get(b"collection".as_ref()),
            attributes.get(b"row".as_ref()),
            attributes.get(b"var".as_ref()),
        ) {
            let var = var.to_str();
            if var != "" {
                let database = self.database.read().unwrap();
                let mut inner = IndexMap::new();
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
                        inner.insert(
                            "row".to_owned(),
                            WildDocValue::Number(serde_json::Number::from(row)),
                        );
                        if let Some(temporary_collection) = session_maybe_has_collection {
                            if let Some(entity) = temporary_collection.get(&row) {
                                inner.insert(
                                    "uuid".to_owned(),
                                    WildDocValue::String(
                                        Uuid::from_u128(entity.uuid()).to_string(),
                                    ),
                                );
                                inner.insert(
                                    "activity".to_owned(),
                                    WildDocValue::Bool(entity.activity() == Activity::Active),
                                );
                                inner.insert(
                                    "term_begin".to_owned(),
                                    WildDocValue::Number(serde_json::Number::from(
                                        entity.term_begin(),
                                    )),
                                );
                                inner.insert(
                                    "term_end".to_owned(),
                                    WildDocValue::Number(serde_json::Number::from(
                                        entity.term_end(),
                                    )),
                                );

                                let mut depends = IndexMap::new();
                                for d in entity.depends() {
                                    let mut depend = IndexMap::new();
                                    depend.insert(
                                        "collection_id".to_owned(),
                                        WildDocValue::Number(serde_json::Number::from(
                                            d.collection_id(),
                                        )),
                                    );
                                    depend.insert(
                                        "row".to_owned(),
                                        WildDocValue::Number(serde_json::Number::from(d.row())),
                                    );

                                    depends
                                        .insert(d.key().to_string(), WildDocValue::Object(depend));
                                }
                                inner.insert("depends".to_owned(), WildDocValue::Object(depends));

                                let mut field = IndexMap::new();
                                if let Some(Some(field_mask)) = attributes.get(b"fields".as_ref()) {
                                    if let WildDocValue::Array(field_mask) = field_mask.as_ref() {
                                        let entities = entity.fields();
                                        for field_name in field_mask {
                                            let field_name = field_name.to_str();
                                            if let Some(bytes) = entities.get(field_name.as_ref()) {
                                                field.insert(
                                                    field_name.to_string(),
                                                    if let Ok(str) = std::str::from_utf8(bytes) {
                                                        WildDocValue::String(str.to_owned())
                                                    } else {
                                                        WildDocValue::Binary(bytes.to_owned())
                                                    },
                                                );
                                            }
                                        }
                                    }
                                } else {
                                    for (field_name, value) in entity.fields() {
                                        field.insert(
                                            field_name.as_str().to_owned(),
                                            if let Ok(str) = std::str::from_utf8(value) {
                                                WildDocValue::String(str.to_owned())
                                            } else {
                                                WildDocValue::Binary(value.to_owned())
                                            },
                                        );
                                    }
                                }
                                inner.insert("field".to_owned(), WildDocValue::Object(field));
                            }
                        } else {
                            if row > 0 {
                                if let Some(collection) =
                                    self.database.read().unwrap().collection(collection_id)
                                {
                                    let row = row as u32;

                                    if let Some(uuid) = collection.uuid_string(row) {
                                        inner.insert("uuid".to_owned(), WildDocValue::String(uuid));
                                    }
                                    if let Some(activity) = collection.activity(row) {
                                        inner.insert(
                                            "activity".to_owned(),
                                            WildDocValue::Bool(activity == Activity::Active),
                                        );
                                    };
                                    if let Some(term_begin) = collection.term_begin(row) {
                                        inner.insert(
                                            "term_begin".to_owned(),
                                            WildDocValue::Number(serde_json::Number::from(
                                                term_begin,
                                            )),
                                        );
                                    }
                                    if let Some(term_end) = collection.term_end(row) {
                                        inner.insert(
                                            "term_end".to_owned(),
                                            WildDocValue::Number(serde_json::Number::from(
                                                term_end,
                                            )),
                                        );
                                    }
                                    if let Some(last_updated) = collection.last_updated(row) {
                                        inner.insert(
                                            "last_updated".to_owned(),
                                            WildDocValue::Number(serde_json::Number::from(
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
                                                WildDocValue::Number(serde_json::Number::from(
                                                    collection_id,
                                                )),
                                            );
                                            depend.insert(
                                                "collection_name".to_owned(),
                                                WildDocValue::String(collection.name().to_owned()),
                                            );
                                            depend.insert(
                                                "row".to_owned(),
                                                WildDocValue::Number(serde_json::Number::from(
                                                    d.row(),
                                                )),
                                            );
                                            depends.insert(
                                                d.key().to_string(),
                                                WildDocValue::Object(depend),
                                            );
                                        }
                                    }
                                    inner.insert(
                                        "depends".to_owned(),
                                        WildDocValue::Object(depends),
                                    );

                                    let mut field = IndexMap::new();
                                    if let Some(Some(field_mask)) =
                                        attributes.get(b"fields".as_ref())
                                    {
                                        if let WildDocValue::Array(field_mask) = field_mask.as_ref()
                                        {
                                            for field_name in field_mask {
                                                let field_name = field_name.to_str();
                                                let bytes = collection
                                                    .field_bytes(row, field_name.as_ref());
                                                field.insert(
                                                    field_name.to_string(),
                                                    if let Ok(str) = std::str::from_utf8(bytes) {
                                                        WildDocValue::String(str.to_owned())
                                                    } else {
                                                        WildDocValue::Binary(bytes.to_owned())
                                                    },
                                                );
                                            }
                                        }
                                    } else {
                                        for field_name in collection.field_names() {
                                            let bytes = collection.field_bytes(row, field_name);
                                            field.insert(
                                                field_name.clone(),
                                                if let Ok(str) = std::str::from_utf8(bytes) {
                                                    WildDocValue::String(str.to_owned())
                                                } else {
                                                    WildDocValue::Binary(bytes.to_owned())
                                                },
                                            );
                                        }
                                    }

                                    inner.insert("field".to_owned(), WildDocValue::Object(field));
                                }
                            }
                        }
                    }
                }
                json.insert(
                    var.to_string().into_bytes(),
                    Arc::new(RwLock::new(WildDocValue::Object(inner))),
                );
            }
        }
        self.state.stack().write().unwrap().push(json);
    }
}
