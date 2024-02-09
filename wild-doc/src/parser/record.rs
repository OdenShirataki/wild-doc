use std::{
    num::{NonZeroI64, NonZeroU32},
    sync::Arc,
};

use indexmap::IndexMap;
use wild_doc_script::{
    Activity, CollectionRow, FieldName, IncludeAdaptor, Uuid, Vars, WildDocValue,
};

use super::Parser;
use crate::r#const::*;

impl<I: IncludeAdaptor + Send> Parser<I> {
    pub(super) fn record(&self, vars: Vars) -> Vars {
        let mut r = Vars::new();

        if let (Some(collection), Some(row), Some(var)) =
            (vars.get(&*COLLECTION), vars.get(&*ROW), vars.get(&*VAR))
        {
            let var = var.as_string();
            if var.as_str() != "" {
                let mut inner = IndexMap::new();
                if let (Some(collection_id), Ok(row)) = (
                    self.database.read().collection_id(&collection.as_string()),
                    row.as_string().parse::<NonZeroI64>(),
                ) {
                    inner.insert(Arc::clone(&ROW), WildDocValue::Number(row.get().into()));
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
                                        Arc::clone(&UUID),
                                        WildDocValue::String(Arc::new(
                                            Uuid::from_u128(entity.uuid()).to_string(),
                                        )),
                                    ),
                                    (
                                        Arc::clone(&ACTIVITY),
                                        WildDocValue::Bool(entity.activity() == Activity::Active),
                                    ),
                                    (
                                        Arc::clone(&TERM_BEGIN),
                                        WildDocValue::Number(entity.term_begin().into()),
                                    ),
                                    (
                                        Arc::clone(&TERM_END),
                                        WildDocValue::Number(entity.term_end().into()),
                                    ),
                                    (
                                        Arc::clone(&DEPENDS),
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
                                                                    Arc::clone(&COLLECTION_ID),
                                                                    WildDocValue::Number(
                                                                        d.collection_id()
                                                                            .get()
                                                                            .into(),
                                                                    ),
                                                                ),
                                                                (
                                                                    Arc::clone(&ROW),
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
                                        Arc::clone(&FIELD),
                                        WildDocValue::Object(
                                            if let Some(field_mask) = vars.get(&*FIELDS) {
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
                                Arc::clone(&SERIAL),
                                WildDocValue::Number((*collection.serial(row)).into()),
                            );

                            if let Some(uuid) = collection.uuid_string(row) {
                                inner.insert(
                                    Arc::clone(&UUID),
                                    WildDocValue::String(Arc::new(uuid)),
                                );
                            }
                            if let Some(activity) = collection.activity(row) {
                                inner.insert(
                                    Arc::clone(&ACTIVITY),
                                    WildDocValue::Bool(activity == Activity::Active),
                                );
                            };
                            if let Some(term_begin) = collection.term_begin(row) {
                                inner.insert(
                                    Arc::clone(&TERM_BEGIN),
                                    WildDocValue::Number((*term_begin).into()),
                                );
                            }
                            if let Some(term_end) = collection.term_end(row) {
                                inner.insert(
                                    Arc::clone(&TERM_END),
                                    WildDocValue::Number((*term_end).into()),
                                );
                            }
                            if let Some(last_updated) = collection.last_updated(row) {
                                inner.insert(
                                    Arc::clone(&LAST_UPDATED),
                                    WildDocValue::Number((*last_updated).into()),
                                );
                            }
                            inner.extend([
                                (
                                    Arc::clone(&DEPENDS),
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
                                                                        Arc::clone(&COLLECTION_ID),
                                                                        WildDocValue::Number(
                                                                            collection_id
                                                                                .get()
                                                                                .into(),
                                                                        ),
                                                                    ),
                                                                    (
                                                                        Arc::clone(
                                                                            &COLLECTION_NAME,
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
                                                                        Arc::clone(&ROW),
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
                                    Arc::clone(&FIELD),
                                    WildDocValue::Object(
                                        if let Some(field_mask) = vars.get(&*FIELDS) {
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
