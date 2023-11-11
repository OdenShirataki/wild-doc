mod custom_sort;

use std::{borrow::Cow, ops::Deref, sync::Arc};

use semilattice_database_session::{search::Search, Order, OrderKey};
use wild_doc_script::{Vars, WildDocValue};

use self::custom_sort::WdCustomSort;

use super::Parser;

impl Parser {
    #[must_use]
    pub(super) async fn result(&mut self, vars: Vars, mut search: Search) -> Vars {
        let mut r = Vars::new();

        if let Some(var) = vars.get("var") {
            let var = var.to_str();
            if var != "" {
                let collection_id = search.collection_id();

                let orders = make_order(
                    &search,
                    &vars
                        .get("sort")
                        .map_or_else(|| Cow::Borrowed(""), |v| v.to_str()),
                );

                let mut rows: Vec<_> = vec![];

                let mut found_session = false;
                for i in (0..self.sessions.len()).rev() {
                    if let Some(state) = self.sessions.get_mut(i) {
                        if state.session.temporary_collection(collection_id).is_some() {
                            found_session = true;
                            rows = state
                                .session
                                .result_with(&mut search, self.database.read().deref(), &orders)
                                .await
                                .into_iter()
                                .map(|row| {
                                    WildDocValue::Object(
                                        [("row".into(), WildDocValue::Number(row.get().into()))]
                                            .into(),
                                    )
                                })
                                .collect();
                            break;
                        }
                    }
                }

                if !found_session {
                    rows = if let Some(v) = search
                        .result(self.database.read().deref())
                        .await
                        .read()
                        .deref()
                    {
                        v.sort(self.database.read().deref(), &orders)
                    } else {
                        vec![]
                    }
                    .into_iter()
                    .map(|row| {
                        WildDocValue::Object(
                            [("row".into(), WildDocValue::Number(row.get().into()))].into(),
                        )
                    })
                    .collect();
                }

                let len = rows.len();

                r.insert(
                    var.into(),
                    WildDocValue::Object(
                        [
                            (
                                "collection_id".into(),
                                WildDocValue::Number(collection_id.get().into()),
                            ),
                            ("rows".into(), WildDocValue::Array(rows)),
                            ("len".into(), WildDocValue::Number(len.into())),
                        ]
                        .into(),
                    ),
                );
            }
        }

        r
    }
}

fn make_order(search: &Search, sort: &str) -> Vec<Order> {
    let mut orders = vec![];
    if sort.len() > 0 {
        for o in sort.trim().split(",") {
            let o = o.trim();
            let is_desc = o.ends_with(" DESC");
            let o_split: Vec<_> = o.split(" ").collect();
            let field = o_split[0];
            if let Some(order_key) = if field.starts_with("field.") {
                field
                    .strip_prefix("field.")
                    .map(|v| OrderKey::Field(v.into()))
            } else if field.starts_with("join.") {
                field.strip_prefix("join.").map(|v| -> OrderKey {
                    let s: Vec<_> = v.split(".").collect();
                    OrderKey::Custom(Box::new(WdCustomSort {
                        result: Arc::clone(search.get_result()),
                        join_name: s[0].into(),
                        property: s[1].into(),
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
