use std::{
    num::{NonZeroI64, NonZeroU32},
    sync::Arc,
};

use hashbrown::HashMap;
use indexmap::IndexMap;
use semilattice_database_session::{Activity, CollectionRow, Uuid};
use wild_doc_script::WildDocValue;

use super::{AttributeMap, Parser};

impl Parser {
    pub(super) fn record(&self, attributes: AttributeMap) {
        let mut json = HashMap::new();

        if let (Some(Some(collection)), Some(Some(row)), Some(Some(var))) = (
            attributes.get("collection"),
            attributes.get("row"),
            attributes.get("var"),
        ) {
            let var = var.to_str();
            if var != "" {
                let mut inner = IndexMap::new();
                if let (Some(collection_id), Ok(row)) = (
                    self.database.read().collection_id(&collection.to_str()),
                    row.to_str().parse::<NonZeroI64>(),
                ) {
                    inner.insert(
                        "row".into(),
                        Arc::new(WildDocValue::Number(row.get().into())),
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
                                        "uuid".into(),
                                        Arc::new(WildDocValue::String(
                                            Uuid::from_u128(entity.uuid()).to_string(),
                                        )),
                                    ),
                                    (
                                        "activity".into(),
                                        Arc::new(WildDocValue::Bool(
                                            entity.activity() == Activity::Active,
                                        )),
                                    ),
                                    (
                                        "term_begin".into(),
                                        Arc::new(WildDocValue::Number(entity.term_begin().into())),
                                    ),
                                    (
                                        "term_end".into(),
                                        Arc::new(WildDocValue::Number(entity.term_end().into())),
                                    ),
                                    (
                                        "depends".into(),
                                        Arc::new(WildDocValue::Object(
                                            entity
                                                .depends()
                                                .into_iter()
                                                .map(|d| {
                                                    (
                                                        d.key().into(),
                                                        Arc::new(WildDocValue::Object(
                                                            [
                                                                (
                                                                    "collection_id".into(),
                                                                    Arc::new(WildDocValue::Number(
                                                                        d.collection_id()
                                                                            .get()
                                                                            .into(),
                                                                    )),
                                                                ),
                                                                (
                                                                    "row".into(),
                                                                    Arc::new(WildDocValue::Number(
                                                                        d.row().get().into(),
                                                                    )),
                                                                ),
                                                            ]
                                                            .into(),
                                                        )),
                                                    )
                                                })
                                                .collect(),
                                        )),
                                    ),
                                    (
                                        "field".into(),
                                        Arc::new(WildDocValue::Object(
                                            if let Some(Some(field_mask)) = attributes.get("fields")
                                            {
                                                if let WildDocValue::Array(field_mask) =
                                                    field_mask.as_ref()
                                                {
                                                    let entities = entity.fields();
                                                    field_mask
                                                        .into_iter()
                                                        .map(|field_name| {
                                                            let field_name = field_name.to_str();
                                                            if let Some(bytes) =
                                                                entities.get(field_name.as_ref())
                                                            {
                                                                Some((
                                                                    field_name.into(),
                                                                    Arc::new(
                                                                        if let Ok(str) =
                                                                            std::str::from_utf8(
                                                                                bytes,
                                                                            )
                                                                        {
                                                                            WildDocValue::String(
                                                                                str.into(),
                                                                            )
                                                                        } else {
                                                                            WildDocValue::Binary(
                                                                                bytes.to_owned(),
                                                                            )
                                                                        },
                                                                    ),
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
                                                    .into_iter()
                                                    .map(|(field_name, value)| {
                                                        (
                                                            field_name.into(),
                                                            Arc::new(
                                                                if let Ok(str) =
                                                                    std::str::from_utf8(value)
                                                                {
                                                                    WildDocValue::String(str.into())
                                                                } else {
                                                                    WildDocValue::Binary(
                                                                        value.to_owned(),
                                                                    )
                                                                },
                                                            ),
                                                        )
                                                    })
                                                    .collect()
                                            },
                                        )),
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
                                inner.insert("uuid".into(), Arc::new(WildDocValue::String(uuid)));
                            }
                            if let Some(activity) = collection.activity(row) {
                                inner.insert(
                                    "activity".into(),
                                    Arc::new(WildDocValue::Bool(activity == Activity::Active)),
                                );
                            };
                            if let Some(term_begin) = collection.term_begin(row) {
                                inner.insert(
                                    "term_begin".into(),
                                    Arc::new(WildDocValue::Number(term_begin.into())),
                                );
                            }
                            if let Some(term_end) = collection.term_end(row) {
                                inner.insert(
                                    "term_end".into(),
                                    Arc::new(WildDocValue::Number(term_end.into())),
                                );
                            }
                            if let Some(last_updated) = collection.last_updated(row) {
                                inner.insert(
                                    "last_updated".into(),
                                    Arc::new(WildDocValue::Number(last_updated.into())),
                                );
                            }
                            inner.extend([
                                (
                                    "depends".into(),
                                    Arc::new(WildDocValue::Object(
                                        self.database
                                            .read()
                                            .relation()
                                            .depends(None, &CollectionRow::new(collection_id, row))
                                            .into_iter()
                                            .map(|d| {
                                                let collection_id = d.collection_id();
                                                self.database.read().collection(collection_id).map(
                                                    |collection| {
                                                        (
                                                            d.key().into(),
                                                            Arc::new(WildDocValue::Object(
                                                                [
                                                                    (
                                                                        "collection_id".into(),
                                                                        Arc::new(
                                                                            WildDocValue::Number(
                                                                                collection_id
                                                                                    .get()
                                                                                    .into(),
                                                                            ),
                                                                        ),
                                                                    ),
                                                                    (
                                                                        "collection_name".into(),
                                                                        Arc::new(
                                                                            WildDocValue::String(
                                                                                collection
                                                                                    .name()
                                                                                    .into(),
                                                                            ),
                                                                        ),
                                                                    ),
                                                                    (
                                                                        "row".into(),
                                                                        Arc::new(
                                                                            WildDocValue::Number(
                                                                                d.row()
                                                                                    .get()
                                                                                    .into(),
                                                                            ),
                                                                        ),
                                                                    ),
                                                                ]
                                                                .into(),
                                                            )),
                                                        )
                                                    },
                                                )
                                            })
                                            .flatten()
                                            .collect(),
                                    )),
                                ),
                                (
                                    "field".to_owned(),
                                    Arc::new(WildDocValue::Object(
                                        if let Some(Some(field_mask)) = attributes.get("fields") {
                                            if let WildDocValue::Array(field_mask) =
                                                field_mask.as_ref()
                                            {
                                                field_mask
                                                    .into_iter()
                                                    .map(|field_name| {
                                                        let field_name = field_name.to_str();
                                                        let bytes = collection
                                                            .field_bytes(row, field_name.as_ref());
                                                        (
                                                            field_name.into(),
                                                            Arc::new(
                                                                if let Ok(str) =
                                                                    std::str::from_utf8(bytes)
                                                                {
                                                                    WildDocValue::String(str.into())
                                                                } else {
                                                                    WildDocValue::Binary(
                                                                        bytes.into(),
                                                                    )
                                                                },
                                                            ),
                                                        )
                                                    })
                                                    .collect()
                                            } else {
                                                IndexMap::new()
                                            }
                                        } else {
                                            collection
                                                .field_names()
                                                .into_iter()
                                                .map(|field_name| {
                                                    let bytes =
                                                        collection.field_bytes(row, field_name);
                                                    (
                                                        field_name.into(),
                                                        Arc::new(
                                                            if let Ok(str) =
                                                                std::str::from_utf8(bytes)
                                                            {
                                                                WildDocValue::String(str.into())
                                                            } else {
                                                                WildDocValue::Binary(bytes.into())
                                                            },
                                                        ),
                                                    )
                                                })
                                                .collect()
                                        },
                                    )),
                                ),
                            ]);
                        }
                    }
                }
                json.insert(var.into(), Arc::new(WildDocValue::Object(inner)));
            }
        }
        self.state.stack().lock().push(json);
    }
}
