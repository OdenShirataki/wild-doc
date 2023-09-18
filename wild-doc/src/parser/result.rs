mod custom_sort;

use bson::{Bson, Document};
use semilattice_database_session::{search::Search, Order, OrderKey};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use self::custom_sort::WdCustomSort;

use super::{AttributeMap, Parser};

impl Parser {
    #[inline(always)]
    pub(super) fn result(
        &mut self,
        attributes: &AttributeMap,
        search_map: &HashMap<String, Arc<RwLock<Search>>>,
    ) {
        let mut bsons = HashMap::new();
        if let (Some(Some(search)), Some(Some(var))) = (
            attributes.get(b"search".as_ref()),
            attributes.get(b"var".as_ref()),
        ) {
            if let (Some(search), Some(var)) = (search.as_str(), var.as_str()) {
                if search != "" && var != "" {
                    let mut bson_inner = Document::new();
                    if let Some(search) = search_map.get(search) {
                        let collection_id = search.read().unwrap().collection_id();
                        bson_inner.insert("collection_id", collection_id);
                        let orders = make_order(
                            search,
                            &attributes
                                .get(b"sort".as_ref())
                                .and_then(|v| v.as_ref())
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_owned(),
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
                        let bson_rows: Vec<_> = session_maybe_has_collection.map_or_else(
                            || {
                                search
                                    .write()
                                    .unwrap()
                                    .result(&self.database.read().unwrap())
                                    .read()
                                    .unwrap()
                                    .as_ref()
                                    .map_or(vec![], |v| {
                                        v.sort(&self.database.read().unwrap(), &orders)
                                    })
                                    .iter()
                                    .map(|row| {
                                        Bson::Document({
                                            let mut bson_row = Document::new();
                                            bson_row.insert("row", row);
                                            bson_row
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
                                        let mut bson_row = Document::new();
                                        bson_row.insert("row", row);
                                        Bson::Document(bson_row)
                                    })
                                    .collect()
                            },
                        );
                        let len = bson_rows.len();
                        bson_inner.insert("rows", bson_rows);
                        bson_inner.insert("len", len as i64);

                        bsons.insert(
                            var.as_bytes().to_vec(),
                            Arc::new(RwLock::new(Bson::Document(bson_inner))),
                        );
                    }
                }
            }
        }
        self.state.stack().write().unwrap().push(bsons);
    }
}

#[inline(always)]
fn make_order<'a>(search: &Arc<RwLock<Search>>, sort: &str) -> Vec<Order> {
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
