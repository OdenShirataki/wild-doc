mod join;
mod result;

use std::{
    num::{NonZeroI32, NonZeroI64},
    str::FromStr,
    sync::Arc,
};

use anyhow::Result;
use async_recursion::async_recursion;
use chrono::DateTime;
use futures::FutureExt;
use hashbrown::HashMap;
use maybe_xml::{token::Ty, Lexer};
use semilattice_database_session::{
    search::{self, Join, Search},
    Activity, CollectionRow, Condition, Uuid,
};
use wild_doc_script::Vars;

use crate::xml_util;

use super::Parser;

impl Parser {
    fn collection_id(&self, vars: &Vars) -> Option<NonZeroI32> {
        if let Some(collection_name) = vars.get("collection") {
            let collection_name = collection_name.to_str();
            if let Some(collection_id) = self.database.read().collection_id(&collection_name) {
                return Some(collection_id);
            }
            if collection_name != "" {
                if let Some(value) = vars.get("create_collection_if_not_exists") {
                    if value.as_bool().map_or(false, |v| *v) {
                        return Some(
                            self.database
                                .write()
                                .collection_id_or_create(&collection_name),
                        );
                    }
                }
            }
        }
        None
    }

    pub(crate) async fn search(
        &mut self,
        xml: &[u8],
        pos: &mut usize,
        attr: Vars,
    ) -> Result<Vec<u8>> {
        if let Some(collection_id) = self.collection_id(&attr) {
            let (condition, join, result_info) = self.make_conditions(xml, pos, &attr).await;
            if let Some(result_info) = result_info {
                let mut new_vars = Vars::new();
                new_vars.extend(
                    self.result(result_info.0, Search::new(collection_id, condition, join))
                        .await,
                );
                let mut pos = 0;
                self.stack.push(new_vars);
                let r = self.parse(result_info.1, &mut pos).await;
                self.stack.pop();
                return r;
            }
        } else {
            xml_util::to_end(xml, pos);
        }
        Ok(vec![])
    }

    async fn make_conditions<'a>(
        &mut self,
        xml: &'a [u8],
        pos: &mut usize,
        attr: &Vars,
    ) -> (
        Vec<Condition>,
        HashMap<String, Join>,
        Option<(Vars, &'a [u8])>,
    ) {
        let (mut conditions, join, result_info) = self.condition_loop(xml, pos).await;

        if let Some(activity) = attr.get("activity") {
            conditions.push(Condition::Activity(if activity.to_str() == "inactive" {
                Activity::Inactive
            } else {
                Activity::Active
            }));
        }
        if let Some(term) = attr.get("term") {
            let term = term.to_str();
            if term != "all" {
                let term: Vec<_> = term.split('@').collect();
                conditions.push(Condition::Term(if term.len() == 2 {
                    DateTime::parse_from_str(term[1], "%Y-%m-%d %H:%M:%S").map_or_else(
                        |_| search::Term::default(),
                        |t| match term[0] {
                            "in" => search::Term::In(t.timestamp() as u64),
                            "future" => search::Term::Future(t.timestamp() as u64),
                            "past" => search::Term::Past(t.timestamp() as u64),
                            _ => search::Term::default(),
                        },
                    )
                } else {
                    search::Term::default()
                }));
            }
        }
        (conditions, join, result_info)
    }

    #[async_recursion(?Send)]
    async fn condition_loop<'a>(
        &mut self,
        xml: &'a [u8],
        pos: &mut usize,
    ) -> (
        Vec<Condition>,
        HashMap<String, Join>,
        Option<(Vars, &'a [u8])>,
    ) {
        let mut join = HashMap::new();
        let mut result_conditions = Vec::new();
        let mut result_info = None;

        let mut futs = vec![];

        let mut deps = 0;
        let lexer = unsafe { Lexer::from_slice_unchecked(xml) };

        while let Some(token) = lexer.tokenize(pos) {
            match token.ty() {
                Ty::StartTag(st) => {
                    deps += 1;
                    let name = st.name();
                    if let None = name.namespace_prefix() {
                        match name.local().as_bytes() {
                            b"narrow" => {
                                if let Ok(inner_xml) = self.parse(xml, pos).await.as_mut() {
                                    let mut _pos = 0;
                                    let (cond, _, _) =
                                        self.condition_loop(&inner_xml, &mut _pos).await;
                                    result_conditions.push(Condition::Narrow(cond));
                                }
                            }
                            b"wide" => {
                                if let Ok(inner_xml) = self.parse(xml, pos).await {
                                    let _pos = 0;
                                    let (cond, _, _) = self.condition_loop(&inner_xml, pos).await;
                                    result_conditions.push(Condition::Wide(cond));
                                }
                            }
                            b"join" => {
                                let attr = self.vars_from_attibutes(st.attributes()).await;
                                self.join(xml, pos, &attr, &mut join).await;
                            }
                            b"result" => {
                                let attr = self.vars_from_attibutes(st.attributes()).await;
                                let begin = *pos;
                                let (inner, _) = xml_util::to_end(xml, pos);
                                result_info = Some((attr, &xml[begin..inner]));
                            }
                            _ => {}
                        }
                    }
                }
                Ty::EmptyElementTag(eet) => {
                    let attr = self.vars_from_attibutes(eet.attributes()).await;
                    let name = eet.name();
                    match name.local().as_bytes() {
                        b"row" => futs.push(Self::condition_row(attr).boxed_local()),
                        b"field" => futs.push(Self::condition_field(attr).boxed_local()),
                        b"uuid" => futs.push(Self::condition_uuid(attr).boxed_local()),
                        b"depend" => {
                            if let Some(c) = self.condition_depend(attr).await {
                                result_conditions.push(c);
                            }
                        }
                        _ => {}
                    }
                }
                Ty::EndTag(_) => {
                    deps -= 1;
                    if deps <= 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        result_conditions.extend(futures::future::join_all(futs).await.into_iter().flatten());
        (result_conditions, join, result_info)
    }

    async fn condition_depend(&self, vars: Vars) -> Option<Condition> {
        if let (Some(row), Some(collection_name)) = (vars.get("row"), vars.get("collection")) {
            let row = row.to_str();
            let collection_name = collection_name.to_str();
            if row != "" && collection_name != "" {
                if let (Ok(row), Some(collection_id)) = (
                    row.parse::<NonZeroI64>(),
                    self.database.read().collection_id(&collection_name),
                ) {
                    return Some(Condition::Depend(
                        vars.get("key").map(|v| v.to_str().into()),
                        if row.get() < 0 {
                            CollectionRow::new(-collection_id, (-row).try_into().unwrap())
                        } else {
                            CollectionRow::new(collection_id, row.try_into().unwrap())
                        },
                    ));
                }
            }
        }
        None
    }

    async fn condition_row(vars: Vars) -> Option<Condition> {
        if let (Some(method), Some(value)) = (vars.get("method"), vars.get("value")) {
            let value = value.to_str();
            if value != "" {
                let method = method.to_str();
                match method.as_ref() {
                    "in" => {
                        let v: Vec<_> = value.split(',').flat_map(|s| s.parse::<isize>()).collect();
                        if v.len() > 0 {
                            return Some(Condition::Row(search::Number::In(v)));
                        }
                    }
                    "min" => {
                        if let Ok(v) = value.parse::<isize>() {
                            return Some(Condition::Row(search::Number::Min(v)));
                        }
                    }
                    "max" => {
                        if let Ok(v) = value.parse::<isize>() {
                            return Some(Condition::Row(search::Number::Max(v)));
                        }
                    }
                    "range" => {
                        let s: Vec<_> = value.split("..").collect();
                        if s.len() == 2 {
                            if let (Ok(min), Ok(max)) = (s[0].parse::<u32>(), s[1].parse::<u32>()) {
                                return Some(Condition::Row(search::Number::Range(
                                    (min as isize)..=(max as isize),
                                )));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        None
    }

    async fn condition_uuid(vars: Vars) -> Option<Condition> {
        if let Some(value) = vars.get("value") {
            let value = value.to_str();
            (value != "")
                .then(|| {
                    let v: Vec<_> = value
                        .split(',')
                        .flat_map(|s| Uuid::from_str(&s).map(|uuid| uuid.as_u128()))
                        .collect();
                    (v.len() > 0).then(|| Condition::Uuid(v))
                })
                .and_then(|v| v)
        } else {
            None
        }
    }

    async fn condition_field(vars: Vars) -> Option<Condition> {
        if let (Some(name), Some(method), Some(value)) =
            (vars.get("name"), vars.get("method"), vars.get("value"))
        {
            let name = name.to_str();
            let method = method.to_str();
            let value = value.to_str();
            (name != "" && method != "" && value != "")
                .then(|| {
                    let method_pair: Vec<_> = method.split('!').collect();
                    let len = method_pair.len();
                    let i = len - 1;
                    match method_pair[i] {
                        "match" => Some(search::Field::Match(value.to_string().into())),
                        "min" => Some(search::Field::Min(value.to_string().into())),
                        "max" => Some(search::Field::Max(value.to_string().into())),
                        "partial" => Some(search::Field::Partial(Arc::new(value.into()))),
                        "forward" => Some(search::Field::Forward(Arc::new(value.into()))),
                        "backward" => Some(search::Field::Backward(Arc::new(value.into()))),
                        "range" => {
                            let s: Vec<_> = value.split("..").collect();
                            (s.len() == 2).then(|| search::Field::Range(s[0].into(), s[1].into()))
                        }
                        "value_forward" => {
                            Some(search::Field::ValueForward(Arc::new(value.into())))
                        }
                        "value_backward" => {
                            Some(search::Field::ValueBackward(Arc::new(value.into())))
                        }
                        "value_partial" => {
                            Some(search::Field::ValuePartial(Arc::new(value.into())))
                        }
                        _ => None,
                    }
                    .map(|method| Condition::Field(name.into(), method))
                })
                .and_then(|v| v)
        } else {
            None
        }
    }
}
