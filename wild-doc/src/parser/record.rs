use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use semilattice_database_session::{Activity, CollectionRow, Uuid};
use serde_json::{json, Map, Value};
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
            let var = var.as_bytes();
            if var.as_ref() != b"" {
                let database = self.database.read().unwrap();
                let mut json_inner = Map::new();
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
                        json_inner.insert("row".to_owned(), json!(row));
                        if let Some(temporary_collection) = session_maybe_has_collection {
                            if let Some(entity) = temporary_collection.get(&row) {
                                json_inner.insert(
                                    "uuid".to_owned(),
                                    json!(Uuid::from_u128(entity.uuid()).to_string()),
                                );
                                json_inner.insert(
                                    "activity".to_owned(),
                                    json!(entity.activity() == Activity::Active),
                                );
                                json_inner
                                    .insert("term_begin".to_owned(), json!(entity.term_begin()));
                                json_inner
                                    .insert("term_begin".to_owned(), json!(entity.term_end()));

                                let mut json_depends = Map::new();
                                for d in entity.depends() {
                                    let mut json_depend = Map::new();
                                    json_depend.insert(
                                        "collection_id".to_owned(),
                                        json!(d.collection_id()),
                                    );
                                    json_depend.insert("row".to_owned(), json!(d.row()));

                                    json_depends
                                        .insert(d.key().to_string(), Value::Object(json_depend));
                                }
                                json_inner
                                    .insert("depends".to_owned(), Value::Object(json_depends));

                                let mut json_field = Map::new();
                                for (field_name, value) in entity.fields() {
                                    json_field.insert(
                                        field_name.as_str().to_owned(),
                                        serde_json::from_slice(value).unwrap_or_default(),
                                    );
                                }
                                json_inner.insert("field".to_owned(), Value::Object(json_field));
                            }
                        } else {
                            if row > 0 {
                                if let Some(collection) =
                                    self.database.read().unwrap().collection(collection_id)
                                {
                                    let row = row as u32;

                                    if let Some(uuid) = collection.uuid_string(row) {
                                        json_inner.insert("uuid".to_owned(), json!(uuid));
                                    }
                                    if let Some(activity) = collection.activity(row) {
                                        json_inner.insert(
                                            "activity".to_owned(),
                                            json!(activity == Activity::Active),
                                        );
                                    };
                                    if let Some(term_begin) = collection.term_begin(row) {
                                        json_inner
                                            .insert("term_begin".to_owned(), json!(term_begin));
                                    }
                                    if let Some(term_end) = collection.term_end(row) {
                                        json_inner.insert("term_end".to_owned(), json!(term_end));
                                    }
                                    if let Some(last_updated) = collection.last_updated(row) {
                                        json_inner
                                            .insert("last_updated".to_owned(), json!(last_updated));
                                    }
                                    let mut json_depends = Map::new();
                                    for d in self
                                        .database
                                        .read()
                                        .unwrap()
                                        .relation()
                                        .read()
                                        .unwrap()
                                        .depends(None, &CollectionRow::new(collection_id, row))
                                    {
                                        let mut json_depend = Map::new();

                                        let collection_id = d.collection_id();

                                        if let Some(collection) =
                                            self.database.read().unwrap().collection(collection_id)
                                        {
                                            json_depend.insert(
                                                "collection_id".to_owned(),
                                                json!(collection_id),
                                            );
                                            json_depend.insert(
                                                "collection_name".to_owned(),
                                                json!(collection.name()),
                                            );
                                            json_depend.insert("row".to_owned(), json!(d.row()));
                                            json_depends.insert(
                                                d.key().to_string(),
                                                Value::Object(json_depend),
                                            );
                                        }
                                    }
                                    json_inner
                                        .insert("depends".to_owned(), Value::Object(json_depends));

                                    let mut json_field = Map::new();
                                    for field_name in &collection.field_names() {
                                        json_field.insert(
                                            field_name.as_str().to_owned(),
                                            serde_json::from_slice(
                                                collection.field_bytes(row, field_name),
                                            )
                                            .unwrap_or_default(),
                                        );
                                    }
                                    json_inner
                                        .insert("field".to_owned(), Value::Object(json_field));
                                }
                            }
                        }
                    }
                }
                json.insert(
                    var.to_vec(),
                    Arc::new(RwLock::new(WildDocValue::from(Value::Object(json_inner)))),
                );
            }
        }
        self.state.stack().write().unwrap().push(json);
    }
}
