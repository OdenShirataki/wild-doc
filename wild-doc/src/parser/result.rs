use semilattice_database_session::{
    anyhow, Activity, CollectionRow, Condition, Order, OrderKey, Uuid,
};
use std::{collections::HashMap, sync::Arc};

use super::{AttributeMap, Parser, WildDocValue};

impl Parser {
    pub(super) fn result(
        &mut self,
        attributes: &AttributeMap,
        search_map: &HashMap<String, (i32, Vec<Condition>)>,
    ) -> anyhow::Result<()> {
        let mut json = HashMap::new();
        if let (Some(Some(search)), Some(Some(var))) = (
            attributes.get(b"search".as_ref()),
            attributes.get(b"var".as_ref()),
        ) {
            let search = search.to_str();
            let var = var.to_str();
            if search != "" && var != "" {
                let mut json_inner = serde_json::Map::new();
                if let Some((collection_id, conditions)) = search_map.get(search.as_ref()) {
                    let collection_id = *collection_id;
                    json_inner.insert("collection_id".to_owned(), serde_json::json!(collection_id));
                    let orders = make_order(
                        if let Some(Some(sort)) = attributes.get(b"sort".as_ref()) {
                            sort.to_str()
                        } else {
                            "".into()
                        }
                        .as_ref(),
                    );

                    let mut session_maybe_has_collection = None;
                    for i in (0..self.sessions.len()).rev() {
                        if let Some(_) = self.sessions[i].0.temporary_collection(collection_id) {
                            session_maybe_has_collection = Some(&self.sessions[i].0);
                            break;
                        }
                    }
                    let json_rows = if let Some(collection) = self
                        .database
                        .clone()
                        .read()
                        .unwrap()
                        .collection(collection_id)
                    {
                        if let Some(session) = session_maybe_has_collection {
                            self.database
                                .clone()
                                .read()
                                .unwrap()
                                .result_session(session.search(collection_id, conditions), orders)?
                                .iter()
                                .map(|row| {
                                    let mut json_row = serde_json::Map::new();
                                    if let Some(temporary_collection) =
                                        session.temporary_collection(collection_id)
                                    {
                                        if let Some(entity) = temporary_collection.get(row) {
                                            json_row
                                                .insert("row".to_owned(), serde_json::json!(row));
                                            json_row.insert(
                                                "uuid".to_owned(),
                                                serde_json::json!(
                                                    Uuid::from_u128(entity.uuid()).to_string()
                                                ),
                                            );
                                            json_row.insert(
                                                "activity".to_owned(),
                                                serde_json::json!(
                                                    entity.activity() == Activity::Active
                                                ),
                                            );
                                            json_row.insert(
                                                "term_begin".to_owned(),
                                                serde_json::json!(entity.term_begin()),
                                            );
                                            json_row.insert(
                                                "term_begin".to_owned(),
                                                serde_json::json!(entity.term_end()),
                                            );

                                            let mut json_depends = serde_json::Map::new();
                                            for d in entity.depends() {
                                                let mut json_depend = serde_json::Map::new();
                                                json_depend.insert(
                                                    "collection_id".to_owned(),
                                                    serde_json::json!(d.collection_id()),
                                                );
                                                json_depend.insert(
                                                    "row".to_owned(),
                                                    serde_json::json!(d.row()),
                                                );

                                                json_depends.insert(
                                                    d.key().to_string(),
                                                    serde_json::Value::Object(json_depend),
                                                );
                                            }
                                            json_row.insert(
                                                "depends".to_owned(),
                                                serde_json::Value::Object(json_depends),
                                            );

                                            let mut json_field = serde_json::Map::new();
                                            for (field_name, value) in entity.fields() {
                                                if let Ok(value) = std::str::from_utf8(value) {
                                                    json_field.insert(
                                                        field_name.as_str().to_owned(),
                                                        serde_json::json!(value),
                                                    );
                                                }
                                            }
                                            json_row.insert(
                                                "field".to_owned(),
                                                serde_json::Value::Object(json_field),
                                            );
                                        }
                                    }
                                    serde_json::Value::Object(json_row)
                                })
                                .collect::<Vec<serde_json::Value>>()
                        } else {
                            let mut search = self.database.read().unwrap().search(collection);
                            for c in conditions {
                                search = search.search(c.clone());
                            }
                            self.database
                                .clone()
                                .read()
                                .unwrap()
                                .result(search, &orders)?
                                .iter()
                                .map(|row| {
                                    let row = *row;
                                    let mut json_row = serde_json::Map::new();
                                    json_row.insert("row".to_owned(), serde_json::json!(row));
                                    json_row.insert(
                                        "uuid".to_owned(),
                                        serde_json::json!(
                                            Uuid::from_u128(collection.uuid(row)).to_string()
                                        ),
                                    );
                                    json_row.insert(
                                        "activity".to_owned(),
                                        serde_json::json!(
                                            collection.activity(row) == Activity::Active
                                        ),
                                    );
                                    json_row.insert(
                                        "term_begin".to_owned(),
                                        serde_json::json!(collection.term_begin(row)),
                                    );
                                    json_row.insert(
                                        "term_end".to_owned(),
                                        serde_json::json!(collection.term_end(row)),
                                    );

                                    let mut json_depends = serde_json::Map::new();
                                    for d in self
                                        .database
                                        .read()
                                        .unwrap()
                                        .relation()
                                        .depends(None, &CollectionRow::new(collection_id, row))
                                    {
                                        let mut json_depend = serde_json::Map::new();

                                        let collection_id = d.collection_id();

                                        if let Some(collection) =
                                            self.database.read().unwrap().collection(collection_id)
                                        {
                                            json_depend.insert(
                                                "collection_id".to_owned(),
                                                serde_json::json!(collection_id),
                                            );
                                            json_depend.insert(
                                                "collection_name".to_owned(),
                                                serde_json::json!(collection.name()),
                                            );
                                            json_depend.insert(
                                                "row".to_owned(),
                                                serde_json::json!(d.row()),
                                            );
                                            json_depends.insert(
                                                d.key().to_string(),
                                                serde_json::Value::Object(json_depend),
                                            );
                                        }
                                    }
                                    json_row.insert(
                                        "depends".to_owned(),
                                        serde_json::Value::Object(json_depends),
                                    );

                                    let mut json_field = serde_json::Map::new();
                                    for field_name in &collection.field_names() {
                                        if let Ok(value) = std::str::from_utf8(
                                            collection.field_bytes(row, field_name),
                                        ) {
                                            json_field.insert(
                                                field_name.as_str().to_owned(),
                                                serde_json::json!(value),
                                            );
                                        }
                                    }
                                    json_row.insert(
                                        "field".to_owned(),
                                        serde_json::Value::Object(json_field),
                                    );
                                    serde_json::Value::Object(json_row)
                                })
                                .collect::<Vec<serde_json::Value>>()
                        }
                    } else {
                        Vec::<serde_json::Value>::new()
                    };
                    let len = json_rows.len();
                    json_inner.insert("rows".to_owned(), serde_json::Value::Array(json_rows));
                    json_inner.insert("len".to_owned(), serde_json::json!(len));

                    json.insert(
                        var.as_bytes().to_vec(),
                        Arc::new(WildDocValue::new(serde_json::Value::Object(json_inner))),
                    );
                }
            }
        }
        self.state.stack().write().unwrap().push(json);
        Ok(())
    }
}

fn make_order(sort: &str) -> Vec<Order> {
    let mut orders = vec![];
    if sort.len() > 0 {
        for o in sort.trim().split(",") {
            let o = o.trim();
            let is_desc = o.ends_with(" DESC");
            let o_split: Vec<&str> = o.split(" ").collect();
            let field = o_split[0];
            let order_key = if field.starts_with("field.") {
                if let Some(field_name) = field.strip_prefix("field.") {
                    Some(OrderKey::Field(field_name.to_owned()))
                } else {
                    None
                }
            } else {
                match field {
                    "serial" => Some(OrderKey::Serial),
                    "row" => Some(OrderKey::Row),
                    "term_begin" => Some(OrderKey::TermBegin),
                    "term_end" => Some(OrderKey::TermEnd),
                    "last_update" => Some(OrderKey::LastUpdated),
                    _ => None,
                }
            };
            if let Some(order_key) = order_key {
                orders.push(if is_desc {
                    Order::Desc(order_key)
                } else {
                    Order::Asc(order_key)
                });
            }
        }
    }
    orders
}
