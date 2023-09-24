mod custom_sort;

use indexmap::IndexMap;
use semilattice_database_session::{search::Search, Order, OrderKey};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use self::custom_sort::WdCustomSort;

use super::{AttributeMap, Parser, WildDocValue};

impl Parser {
    pub(super) fn result(
        &mut self,
        attributes: &AttributeMap,
        search_map: &HashMap<String, Arc<RwLock<Search>>>,
    ) {
        let mut vars = HashMap::new();
        if let (Some(Some(search)), Some(Some(var))) = (
            attributes.get(b"search".as_ref()),
            attributes.get(b"var".as_ref()),
        ) {
            let search = search.to_str();
            let var = var.to_str();
            if search != "" && var != "" {
                let mut inner = IndexMap::new();
                if let Some(search) = search_map.get(search.as_ref()) {
                    let collection_id = search.read().unwrap().collection_id();
                    inner.insert(
                        "collection_id".to_owned(),
                        WildDocValue::Number(serde_json::Number::from(collection_id)),
                    );
                    let orders = make_order(
                        search,
                        &attributes
                            .get(b"sort".as_ref())
                            .and_then(|v| v.as_ref())
                            .map_or_else(|| "".to_owned(), |v| v.to_string()),
                    );
                    let mut session_maybe_has_collection = None;
                    for i in (0..self.sessions.len()).rev() {
                        if self.sessions[i]
                            .session
                            .temporary_collection(collection_id)
                            .is_some()
                        {
                            session_maybe_has_collection = Some(&self.sessions[i].session);
                            break;
                        }
                    }
                    let rows: Vec<_> = session_maybe_has_collection.map_or_else(
                        || {
                            search
                                .write()
                                .unwrap()
                                .result(&self.database.read().unwrap())
                                .read()
                                .unwrap()
                                .as_ref()
                                .map_or(vec![], |v| v.sort(&self.database.read().unwrap(), &orders))
                                .iter()
                                .map(|row| {
                                    WildDocValue::Object({
                                        let mut r = IndexMap::new();
                                        r.insert(
                                            "row".to_owned(),
                                            WildDocValue::Number(serde_json::Number::from(*row)),
                                        );
                                        r
                                    })
                                })
                                .collect()
                        },
                        |session| {
                            session
                                .search(&search)
                                .result(&self.database.read().unwrap(), &orders)
                                .iter()
                                .map(|row| {
                                    WildDocValue::Object({
                                        let mut r = IndexMap::new();
                                        r.insert(
                                            "row".to_owned(),
                                            WildDocValue::Number(serde_json::Number::from(*row)),
                                        );
                                        r
                                    })
                                })
                                .collect()
                        },
                    );
                    let len = rows.len();
                    inner.insert("rows".to_owned(), WildDocValue::Array(rows));
                    inner.insert(
                        "len".to_owned(),
                        WildDocValue::Number(serde_json::Number::from(len)),
                    );

                    vars.insert(
                        var.to_string().into_bytes(),
                        Arc::new(RwLock::new(WildDocValue::Object(inner))),
                    );
                }
            }
        }
        self.state.stack().write().unwrap().push(vars);
    }
}

fn make_order(search: &Arc<RwLock<Search>>, sort: &str) -> Vec<Order> {
    let mut orders = vec![];
    if sort.len() > 0 {
        for o in sort.trim().split(",") {
            let o = o.trim();
            let is_desc = o.ends_with(" DESC");
            let o_split: Vec<&str> = o.split(" ").collect();
            let field = o_split[0];
            if let Some(order_key) = if field.starts_with("field.") {
                field
                    .strip_prefix("field.")
                    .map(|v| OrderKey::Field(v.to_owned()))
            } else if field.starts_with("join.") {
                field.strip_prefix("join.").map(|v| -> OrderKey {
                    let s: Vec<&str> = v.split(".").collect();
                    OrderKey::Custom(Box::new(WdCustomSort {
                        result: search.read().unwrap().get_result(),
                        join_name: s[0].to_owned(),
                        property: s[1].to_owned(),
                    }))
                })
            } else {
                match field {
                    "serial" => Some(OrderKey::Serial),
                    "row" => Some(OrderKey::Row),
                    "term_begin" => Some(OrderKey::TermBegin),
                    "term_end" => Some(OrderKey::TermEnd),
                    "last_update" => Some(OrderKey::LastUpdated),
                    _ => None,
                }
            } {
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
