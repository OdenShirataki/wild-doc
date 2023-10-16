use std::{
    num::{NonZeroI64, NonZeroU32},
    sync::Arc,
};

use hashbrown::HashMap;
use indexmap::IndexMap;
use semilattice_database_session::{Activity, CollectionRow, Uuid};
use wild_doc_script::WildDocValue;

use super::Parser;

impl Parser {
    pub(super) fn record(&self, attributes: HashMap<Vec<u8>, Option<Arc<WildDocValue>>>) {
        let mut json = HashMap::new();

        if let (Some(Some(collection)), Some(Some(row)), Some(Some(var))) = (
            attributes.get(b"collection".as_ref()),
            attributes.get(b"row".as_ref()),
            attributes.get(b"var".as_ref()),
        ) {
            let var = var.to_str();
            if var != "" {
                let mut inner = IndexMap::new();
                if let (Some(collection_id), Ok(row)) = (
                    self.database.read().collection_id(&collection.to_string()),
                    row.to_string().parse::<NonZeroI64>(),
                ) {
                    inner.insert(
                        "row".to_owned(),
                        WildDocValue::Number(serde_json::Number::from(row.get())),
                    );
                    let mut find_session = false;
                    for i in (0..self.sessions.len()).rev() {
                        if let Some(temporary_collection) = self
                            .sessions
                            .get(i)
                            .and_then(|v| v.session.temporary_collection(collection_id))
                        {
                            find_session = true;
                            if let Some(entity) = temporary_collection.get(&row) {
                                inner.extend([
                                    (
                                        "uuid".to_owned(),
                                        WildDocValue::String(
                                            Uuid::from_u128(entity.uuid()).to_string(),
                                        ),
                                    ),
                                    (
                                        "activity".to_owned(),
                                        WildDocValue::Bool(entity.activity() == Activity::Active),
                                    ),
                                    (
                                        "term_begin".to_owned(),
                                        WildDocValue::Number(serde_json::Number::from(
                                            entity.term_begin(),
                                        )),
                                    ),
                                    (
                                        "term_end".to_owned(),
                                        WildDocValue::Number(serde_json::Number::from(
                                            entity.term_end(),
                                        )),
                                    ),
                                    (
                                        "depends".to_owned(),
                                        WildDocValue::Object(
                                            entity
                                                .depends()
                                                .iter()
                                                .map(|d| {
                                                    (
                                                        d.key().to_string(),
                                                        WildDocValue::Object(IndexMap::from([
                                                            (
                                                                "collection_id".to_owned(),
                                                                WildDocValue::Number(
                                                                    serde_json::Number::from(
                                                                        d.collection_id().get(),
                                                                    ),
                                                                ),
                                                            ),
                                                            (
                                                                "row".to_owned(),
                                                                WildDocValue::Number(
                                                                    serde_json::Number::from(
                                                                        d.row().get(),
                                                                    ),
                                                                ),
                                                            ),
                                                        ])),
                                                    )
                                                })
                                                .collect(),
                                        ),
                                    ),
                                    (
                                        "field".to_owned(),
                                        WildDocValue::Object(
                                            if let Some(Some(field_mask)) =
                                                attributes.get(b"fields".as_ref())
                                            {
                                                if let WildDocValue::Array(field_mask) =
                                                    field_mask.as_ref()
                                                {
                                                    let entities = entity.fields();
                                                    field_mask
                                                        .iter()
                                                        .map(|field_name| {
                                                            let field_name = field_name.to_str();
                                                            if let Some(bytes) =
                                                                entities.get(field_name.as_ref())
                                                            {
                                                                Some((
                                                                    field_name.to_string(),
                                                                    if let Ok(str) =
                                                                        std::str::from_utf8(bytes)
                                                                    {
                                                                        WildDocValue::String(
                                                                            str.to_owned(),
                                                                        )
                                                                    } else {
                                                                        WildDocValue::Binary(
                                                                            bytes.to_owned(),
                                                                        )
                                                                    },
                                                                ))
                                                            } else {
                                                                None
                                                            }
                                                        })
                                                        .flatten()
                                                        .collect()
                                                } else {
                                                    IndexMap::new()
                                                }
                                            } else {
                                                entity
                                                    .fields()
                                                    .iter()
                                                    .map(|(field_name, value)| {
                                                        (
                                                            field_name.as_str().to_owned(),
                                                            if let Ok(str) =
                                                                std::str::from_utf8(value)
                                                            {
                                                                WildDocValue::String(str.to_owned())
                                                            } else {
                                                                WildDocValue::Binary(
                                                                    value.to_owned(),
                                                                )
                                                            },
                                                        )
                                                    })
                                                    .collect()
                                            },
                                        ),
                                    ),
                                ]);
                            }
                            break;
                        }
                    }

                    if !find_session && row.get() > 0 {
                        if let Some(collection) = self.database.read().collection(collection_id) {
                            let row = unsafe { NonZeroU32::new_unchecked(row.get() as u32) };

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
                                    WildDocValue::Number(serde_json::Number::from(term_begin)),
                                );
                            }
                            if let Some(term_end) = collection.term_end(row) {
                                inner.insert(
                                    "term_end".to_owned(),
                                    WildDocValue::Number(serde_json::Number::from(term_end)),
                                );
                            }
                            if let Some(last_updated) = collection.last_updated(row) {
                                inner.insert(
                                    "last_updated".to_owned(),
                                    WildDocValue::Number(serde_json::Number::from(last_updated)),
                                );
                            }
                            inner.extend([
                                (
                                    "depends".to_owned(),
                                    WildDocValue::Object(
                                        self.database
                                            .read()
                                            .relation()
                                            .depends(None, &CollectionRow::new(collection_id, row))
                                            .iter()
                                            .map(|d| {
                                                let collection_id = d.collection_id();
                                                self.database.read().collection(collection_id).map(
                                                    |collection| {
                                                        (
                                                            d.key().to_string(),
                                                            WildDocValue::Object(IndexMap::from([
                                                                (
                                                                    "collection_id".to_owned(),
                                                                    WildDocValue::Number(
                                                                        serde_json::Number::from(
                                                                            collection_id.get(),
                                                                        ),
                                                                    ),
                                                                ),
                                                                (
                                                                    "collection_name".to_owned(),
                                                                    WildDocValue::String(
                                                                        collection
                                                                            .name()
                                                                            .to_owned(),
                                                                    ),
                                                                ),
                                                                (
                                                                    "row".to_owned(),
                                                                    WildDocValue::Number(
                                                                        serde_json::Number::from(
                                                                            d.row().get(),
                                                                        ),
                                                                    ),
                                                                ),
                                                            ])),
                                                        )
                                                    },
                                                )
                                            })
                                            .flatten()
                                            .collect(),
                                    ),
                                ),
                                (
                                    "field".to_owned(),
                                    WildDocValue::Object(
                                        if let Some(Some(field_mask)) =
                                            attributes.get(b"fields".as_ref())
                                        {
                                            if let WildDocValue::Array(field_mask) =
                                                field_mask.as_ref()
                                            {
                                                field_mask
                                                    .iter()
                                                    .map(|field_name| {
                                                        let field_name = field_name.to_str();
                                                        let bytes = collection
                                                            .field_bytes(row, field_name.as_ref());
                                                        (
                                                            field_name.to_string(),
                                                            if let Ok(str) =
                                                                std::str::from_utf8(bytes)
                                                            {
                                                                WildDocValue::String(str.to_owned())
                                                            } else {
                                                                WildDocValue::Binary(
                                                                    bytes.to_owned(),
                                                                )
                                                            },
                                                        )
                                                    })
                                                    .collect()
                                            } else {
                                                IndexMap::new()
                                            }
                                        } else {
                                            collection
                                                .field_names()
                                                .iter()
                                                .map(|field_name| {
                                                    let bytes =
                                                        collection.field_bytes(row, field_name);
                                                    (
                                                        field_name.to_string(),
                                                        if let Ok(str) = std::str::from_utf8(bytes)
                                                        {
                                                            WildDocValue::String(str.to_owned())
                                                        } else {
                                                            WildDocValue::Binary(bytes.to_owned())
                                                        },
                                                    )
                                                })
                                                .collect()
                                        },
                                    ),
                                ),
                            ]);
                        }
                    }
                }
                json.insert(var.to_string().into_bytes(), Arc::new(WildDocValue::Object(inner)));
            }
        }
        self.state.stack().lock().push(json);
    }
}
