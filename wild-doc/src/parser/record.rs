use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use bson::{spec::BinarySubtype, Bson, Document};
use semilattice_database_session::{Activity, CollectionRow, Uuid};

use super::Parser;

impl Parser {
    #[inline(always)]
    pub(super) fn record(&mut self, attributes: HashMap<Vec<u8>, Option<Arc<Bson>>>) {
        let mut bsons = HashMap::new();

        if let (Some(Some(collection)), Some(Some(row)), Some(Some(var))) = (
            attributes.get(b"collection".as_ref()),
            attributes.get(b"row".as_ref()),
            attributes.get(b"var".as_ref()),
        ) {
            if let (Some(var), Some(collection)) = (var.as_str(), collection.as_str()) {
                if var != "" {
                    let database = self.database.read().unwrap();
                    let mut bson_inner = Document::new();
                    if let Some(collection_id) = database.collection_id(collection) {
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
                            bson_inner.insert("row", row);
                            if let Some(temporary_collection) = session_maybe_has_collection {
                                if let Some(entity) = temporary_collection.get(&row) {
                                    bson_inner
                                        .insert("uuid", Uuid::from_u128(entity.uuid()).to_string());
                                    bson_inner
                                        .insert("activity", entity.activity() == Activity::Active);
                                    bson_inner.insert("term_begin", entity.term_begin() as u32);
                                    bson_inner.insert("term_begin", entity.term_end() as u32);

                                    let mut bson_depends = Document::new();
                                    for d in entity.depends() {
                                        let collection_id = d.collection_id();
                                        if let Some(collection) =
                                            self.database.read().unwrap().collection(collection_id)
                                        {
                                            let mut bson_depend = Document::new();
                                            bson_depend.insert("collection_id", d.collection_id());
                                            bson_depend
                                                .insert("collection_name", collection.name());
                                            bson_depend.insert("row", d.row());
                                            bson_depends.insert(d.key(), bson_depend);
                                        }
                                    }
                                    bson_inner.insert("depends", bson_depends);

                                    let mut json_field = Document::new();
                                    for (field_name, value) in entity.fields() {
                                        if let Ok(str) = std::str::from_utf8(value) {
                                            json_field.insert(field_name, str);
                                        } else {
                                            json_field.insert(
                                                field_name,
                                                Bson::Binary(bson::Binary {
                                                    subtype: BinarySubtype::Generic,
                                                    bytes: value.to_vec(),
                                                }),
                                            );
                                        }
                                    }
                                    bson_inner.insert("field", json_field);
                                }
                            } else {
                                if row > 0 {
                                    if let Some(collection) =
                                        self.database.read().unwrap().collection(collection_id)
                                    {
                                        let row = row as u32;

                                        if let Some(uuid) = collection.uuid_string(row) {
                                            bson_inner.insert("uuid", uuid);
                                        }
                                        if let Some(activity) = collection.activity(row) {
                                            bson_inner
                                                .insert("activity", activity == Activity::Active);
                                        };
                                        if let Some(term_begin) = collection.term_begin(row) {
                                            bson_inner.insert("term_begin", term_begin as u32);
                                        }
                                        if let Some(term_end) = collection.term_end(row) {
                                            bson_inner.insert("term_end", term_end as u32);
                                        }
                                        if let Some(last_updated) = collection.last_updated(row) {
                                            bson_inner.insert("last_updated", last_updated as u32);
                                        }
                                        let mut bson_depends = Document::new();
                                        for d in self
                                            .database
                                            .read()
                                            .unwrap()
                                            .relation()
                                            .read()
                                            .unwrap()
                                            .depends(None, &CollectionRow::new(collection_id, row))
                                        {
                                            let collection_id = d.collection_id();
                                            if let Some(collection) = self
                                                .database
                                                .read()
                                                .unwrap()
                                                .collection(collection_id)
                                            {
                                                let mut bson_depend = Document::new();
                                                bson_depend.insert("collection_id", collection_id);
                                                bson_depend
                                                    .insert("collection_name", collection.name());
                                                bson_depend.insert("row", d.row());
                                                bson_depends.insert(d.key(), bson_depend);
                                            }
                                        }
                                        bson_inner.insert("depends", bson_depends);

                                        let mut bson_field = Document::new();
                                        for field_name in collection.field_names() {
                                            let bytes = collection.field_bytes(row, field_name);

                                            if let Ok(str) = std::str::from_utf8(bytes) {
                                                bson_field.insert(field_name, str);
                                            } else {
                                                bson_field.insert(
                                                    field_name,
                                                    Bson::Binary(bson::Binary {
                                                        subtype: BinarySubtype::Generic,
                                                        bytes: bytes.to_vec(),
                                                    }),
                                                );
                                            }
                                        }
                                        bson_inner.insert("field", bson_field);
                                    }
                                }
                            }
                        }
                    }
                    bsons.insert(
                        var.as_bytes().to_vec(),
                        Arc::new(RwLock::new(Bson::Document(bson_inner))),
                    );
                }
            }
        }
        self.state.stack().write().unwrap().push(bsons);
    }
}
