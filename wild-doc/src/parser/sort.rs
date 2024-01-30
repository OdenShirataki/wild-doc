mod custom_sort;
mod custom_sort_session;

use std::{ops::Deref, sync::Arc};

use anyhow::Result;
use semilattice_database_session::{
    CustomOrderKey, FieldName, Order, SearchResult, SessionOrder, SessionOrderKey,
    SessionSearchResult,
};
use wild_doc_script::{Vars, WildDocValue};

use self::{custom_sort::WdCustomSort, custom_sort_session::WdCustomSortSession};

use super::Parser;

impl Parser {
    pub(crate) async fn sort(
        &mut self,
        xml: &[u8],
        pos: &mut usize,
        attr: Vars,
    ) -> Result<Vec<u8>> {
        let mut vars = Vars::new();
        if let (Some(WildDocValue::String(order)), Some(result), Some(WildDocValue::String(var))) = (
            attr.get(&self.strings.order),
            attr.get(&self.strings.result),
            attr.get(&self.strings.var),
        ) {
            if var.as_str() != "" {
                match result {
                    WildDocValue::SearchResult(result) => {
                        let orders = make_order(result, order);
                        let rows = result
                            .sort(self.database.read().deref(), &orders)
                            .into_iter()
                            .map(|row| WildDocValue::Number(row.get().into()))
                            .collect();
                        vars.insert(Arc::clone(var), WildDocValue::Array(rows));
                    }
                    WildDocValue::SessionSearchResult(result) => {
                        let _orders = make_order_session(result, order);
                    }
                    _ => {}
                }
            }
        }
        self.stack.push(vars);
        let r = self.parse(xml, pos).await;
        self.stack.pop();
        return r;
    }
}

fn make_order(result: &Arc<SearchResult>, sort: &str) -> Vec<Order<WdCustomSort>> {
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
                    .map(|v| CustomOrderKey::Field(FieldName::new(v.into())))
            } else if field.starts_with("join.") {
                field
                    .strip_prefix("join.")
                    .map(|v| -> CustomOrderKey<WdCustomSort> {
                        let s: Vec<_> = v.split(".").collect();
                        CustomOrderKey::Custom(WdCustomSort {
                            result: Arc::clone(result),
                            join_name: s[0].into(),
                            property: s[1].into(),
                        })
                    })
            } else {
                match field {
                    "serial" => Some(CustomOrderKey::Serial),
                    "row" => Some(CustomOrderKey::Row),
                    "term_begin" => Some(CustomOrderKey::TermBegin),
                    "term_end" => Some(CustomOrderKey::TermEnd),
                    "last_update" => Some(CustomOrderKey::LastUpdated),
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

fn make_order_session(
    result: &Arc<SessionSearchResult>,
    sort: &str,
) -> Vec<SessionOrder<WdCustomSortSession>> {
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
                    .map(|v| SessionOrderKey::Field(FieldName::new(v.into())))
            } else if field.starts_with("join.") {
                field
                    .strip_prefix("join.")
                    .map(|v| -> SessionOrderKey<WdCustomSortSession> {
                        let s: Vec<_> = v.split(".").collect();
                        SessionOrderKey::Custom(WdCustomSortSession {
                            result: Arc::clone(result),
                            join_name: s[0].into(),
                            property: s[1].into(),
                        })
                    })
            } else {
                match field {
                    "serial" => Some(SessionOrderKey::Serial),
                    "row" => Some(SessionOrderKey::Row),
                    "term_begin" => Some(SessionOrderKey::TermBegin),
                    "term_end" => Some(SessionOrderKey::TermEnd),
                    "last_update" => Some(SessionOrderKey::LastUpdated),
                    _ => None,
                }
            } {
                orders.push(if is_desc {
                    SessionOrder::Desc(order_key)
                } else {
                    SessionOrder::Asc(order_key)
                });
            }
        }
    }
    orders
}
