mod custom_sort;

use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use hashbrown::HashMap;
use indexmap::IndexMap;
use parking_lot::RwLock;
use semilattice_database_session::{search::Search, Order, OrderKey};

use self::custom_sort::WdCustomSort;

use super::{AttributeMap, Parser, WildDocValue};

impl Parser {
    pub(super) async fn result(
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
                if let Some(search) = search_map.get(search.as_ref()) {
                    let collection_id = search.read().collection_id();

                    let orders = make_order(
                        search,
                        &attributes
                            .get(b"sort".as_ref())
                            .and_then(|v| v.as_ref())
                            .map_or_else(|| "".to_owned(), |v| v.to_string()),
                    );

                    let mut rows: Vec<_> = vec![];

                    let mut found_session = false;
                    for i in (0..self.sessions.len()).rev() {
                        if let Some(state) = self.sessions.get_mut(i) {
                            if state.session.temporary_collection(collection_id).is_some() {
                                found_session = true;
                                rows = state
                                    .session
                                    .result_with(
                                        &mut search.write().deref_mut(),
                                        self.database.read().deref(),
                                        &orders,
                                    )
                                    .await
                                    .iter()
                                    .map(|row| {
                                        WildDocValue::Object(IndexMap::from([(
                                            "row".to_owned(),
                                            WildDocValue::Number(serde_json::Number::from(
                                                row.get(),
                                            )),
                                        )]))
                                    })
                                    .collect();
                                break;
                            }
                        }
                    }

                    if !found_session {
                        rows = if let Some(v) = search
                            .write()
                            .result(self.database.read().deref())
                            .await
                            .read()
                            .deref()
                        {
                            v.sort(self.database.read().deref(), &orders)
                        } else {
                            vec![]
                        }
                        .iter()
                        .map(|row| {
                            WildDocValue::Object(IndexMap::from([(
                                "row".to_owned(),
                                WildDocValue::Number(serde_json::Number::from(row.get())),
                            )]))
                        })
                        .collect();
                    }

                    let len = rows.len();

                    vars.insert(
                        var.to_string().into_bytes(),
                        Arc::new(RwLock::new(WildDocValue::Object(IndexMap::from([
                            (
                                "collection_id".to_owned(),
                                WildDocValue::Number(serde_json::Number::from(collection_id.get())),
                            ),
                            ("rows".to_owned(), WildDocValue::Array(rows)),
                            (
                                "len".to_owned(),
                                WildDocValue::Number(serde_json::Number::from(len)),
                            ),
                        ])))),
                    );
                }
            }
        }
        self.state.stack().lock().push(vars);
    }
}

#[inline(always)]
fn make_order(search: &RwLock<Search>, sort: &str) -> Vec<Order> {
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
                        result: Arc::clone(search.read().get_result()),
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
