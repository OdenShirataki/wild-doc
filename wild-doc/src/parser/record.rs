use std::{
    num::{NonZeroI64, NonZeroU32},
    sync::Arc,
};

use indexmap::IndexMap;
use semilattice_database_session::{Activity, CollectionRow, FieldName, Uuid};
use wild_doc_script::{Vars, WildDocValue};

use super::Parser;

impl Parser {
    pub(super) fn record(&self, vars: Vars) -> Vars {
        let mut r = Vars::new();

        if let (Some(collection), Some(row), Some(var)) = (
            vars.get(&self.strings.collection),
            vars.get(&self.strings.row),
            vars.get(&self.strings.var),
        ) {
            let var = var.as_string();
            if var.as_str() != "" {
                let mut inner = IndexMap::new();
                if let (Some(collection_id), Ok(row)) = (
                    self.database.read().collection_id(&collection.as_string()),
                    row.as_string().parse::<NonZeroI64>(),
                ) {
                    inner.insert(
                        Arc::clone(&self.strings.row),
                        WildDocValue::Number(row.get().into()),
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
                                        Arc::clone(&self.strings.uuid),
                                        WildDocValue::String(Arc::new(
                                            Uuid::from_u128(entity.uuid()).to_string(),
                                        )),
                                    ),
                                    (
                                        Arc::clone(&self.strings.activity),
                                        WildDocValue::Bool(entity.activity() == Activity::Active),
                                    ),
                                    (
                                        Arc::clone(&self.strings.term_begin),
                                        WildDocValue::Number(entity.term_begin().into()),
                                    ),
                                    (
                                        Arc::clone(&self.strings.term_end),
                                        WildDocValue::Number(entity.term_end().into()),
                                    ),
                                    (
                                        Arc::clone(&self.strings.depends),
                                        WildDocValue::Object(
                                            entity
                                                .depends()
                                                .into_iter()
                                                .map(|d| {
                                                    (
                                                        Arc::clone(d.key()),
                                                        WildDocValue::Object(
                                                            [
                                                                (
                                                                    Arc::clone(
                                                                        &self.strings.collection_id,
                                                                    ),
                                                                    WildDocValue::Number(
                                                                        d.collection_id()
                                                                            .get()
                                                                            .into(),
                                                                    ),
                                                                ),
                                                                (
                                                                    Arc::clone(&self.strings.row),
                                                                    WildDocValue::Number(
                                                                        d.row().get().into(),
                                                                    ),
                                                                ),
                                                            ]
                                                            .into(),
                                                        ),
                                                    )
                                                })
                                                .collect(),
                                        ),
                                    ),
                                    (
                                        Arc::clone(&self.strings.field),
                                        WildDocValue::Object(
                                            if let Some(field_mask) = vars.get(&self.strings.fields)
                                            {
                                                if let WildDocValue::Array(field_mask) = field_mask
                                                {
                                                    let entities = entity.fields();
                                                    field_mask
                                                        .into_iter()
                                                        .map(|field_name| {
                                                            let field_name = FieldName::new(
                                                                field_name.to_string(),
                                                            );
                                                            if let Some(bytes) =
                                                                entities.get(&field_name)
                                                            {
                                                                Some((
                                                                    field_name,
                                                                    if let Ok(str) =
                                                                        std::str::from_utf8(bytes)
                                                                    {
                                                                        WildDocValue::String(
                                                                            Arc::new(str.into()),
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
                                                    .into_iter()
                                                    .map(|(field_name, value)| {
                                                        (
                                                            Arc::clone(field_name),
                                                            if let Ok(str) =
                                                                std::str::from_utf8(value)
                                                            {
                                                                WildDocValue::String(Arc::new(
                                                                    str.into(),
                                                                ))
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

                            inner.insert(
                                Arc::clone(&self.strings.serial),
                                WildDocValue::Number(collection.serial(row).into()),
                            );

                            if let Some(uuid) = collection.uuid_string(row) {
                                inner.insert(
                                    Arc::clone(&self.strings.uuid),
                                    WildDocValue::String(Arc::new(uuid)),
                                );
                            }
                            if let Some(activity) = collection.activity(row) {
                                inner.insert(
                                    Arc::clone(&self.strings.activity),
                                    WildDocValue::Bool(activity == Activity::Active),
                                );
                            };
                            if let Some(term_begin) = collection.term_begin(row) {
                                inner.insert(
                                    Arc::clone(&self.strings.term_begin),
                                    WildDocValue::Number(term_begin.into()),
                                );
                            }
                            if let Some(term_end) = collection.term_end(row) {
                                inner.insert(
                                    Arc::clone(&self.strings.term_end),
                                    WildDocValue::Number(term_end.into()),
                                );
                            }
                            if let Some(last_updated) = collection.last_updated(row) {
                                inner.insert(
                                    Arc::clone(&self.strings.last_updated),
                                    WildDocValue::Number(last_updated.into()),
                                );
                            }
                            inner.extend([
                                (
                                    Arc::clone(&self.strings.depends),
                                    WildDocValue::Object(
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
                                                            Arc::clone(d.key()),
                                                            WildDocValue::Object(
                                                                [
                                                                    (
                                                                        Arc::clone(
                                                                            &self
                                                                                .strings
                                                                                .collection_id,
                                                                        ),
                                                                        WildDocValue::Number(
                                                                            collection_id
                                                                                .get()
                                                                                .into(),
                                                                        ),
                                                                    ),
                                                                    (
                                                                        Arc::clone(
                                                                            &self
                                                                                .strings
                                                                                .collection_name,
                                                                        ),
                                                                        WildDocValue::String(
                                                                            Arc::new(
                                                                                collection
                                                                                    .name()
                                                                                    .into(),
                                                                            ),
                                                                        ),
                                                                    ),
                                                                    (
                                                                        Arc::clone(
                                                                            &self.strings.row,
                                                                        ),
                                                                        WildDocValue::Number(
                                                                            d.row().get().into(),
                                                                        ),
                                                                    ),
                                                                ]
                                                                .into(),
                                                            ),
                                                        )
                                                    },
                                                )
                                            })
                                            .flatten()
                                            .collect(),
                                    ),
                                ),
                                (
                                    Arc::clone(&self.strings.field),
                                    WildDocValue::Object(
                                        if let Some(field_mask) = vars.get(&self.strings.fields) {
                                            if let WildDocValue::Array(field_mask) = field_mask {
                                                field_mask
                                                    .into_iter()
                                                    .map(|field_name| {
                                                        let field_name = field_name.as_string();
                                                        let bytes = collection
                                                            .field_bytes(row, &field_name);
                                                        (
                                                            field_name,
                                                            if let Ok(str) =
                                                                std::str::from_utf8(bytes)
                                                            {
                                                                WildDocValue::String(Arc::new(
                                                                    str.into(),
                                                                ))
                                                            } else {
                                                                WildDocValue::Binary(bytes.into())
                                                            },
                                                        )
                                                    })
                                                    .collect()
                                            } else {
                                                IndexMap::new()
                                            }
                                        } else {
                                            collection
                                                .fields()
                                                .into_iter()
                                                .map(|(field_name, _)| {
                                                    let bytes =
                                                        collection.field_bytes(row, field_name);
                                                    (
                                                        Arc::clone(field_name),
                                                        if let Ok(str) = std::str::from_utf8(bytes)
                                                        {
                                                            WildDocValue::String(Arc::new(
                                                                str.into(),
                                                            ))
                                                        } else {
                                                            WildDocValue::Binary(bytes.into())
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
                r.insert(var, WildDocValue::Object(inner));
            }
        }
        r
    }
}
