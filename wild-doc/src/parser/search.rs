mod join;

use std::{
    num::{NonZeroI32, NonZeroI64},
    str::FromStr,
    sync::Arc,
};

use async_recursion::async_recursion;
use chrono::DateTime;
use futures::FutureExt;
use hashbrown::HashMap;
use maybe_xml::{
    scanner::{Scanner, State},
    token,
};
use semilattice_database_session::{
    search::{self, Join, Search},
    Activity, CollectionRow, Condition, Uuid,
};
use wild_doc_script::Vars;

use crate::xml_util;

use super::Parser;

impl Parser {
    #[inline(always)]
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

    pub(crate) async fn search<'a>(
        &mut self,
        xml: &'a [u8],
        vars: Vars,
        search_map: &mut HashMap<String, Search>,
    ) -> &'a [u8] {
        if let Some(name) = vars.get("name") {
            let name = name.to_str();
            if name != "" {
                if let Some(collection_id) = self.collection_id(&vars) {
                    let (last_xml, condition, join) = self.make_conditions(&vars, xml).await;
                    search_map.insert(
                        name.into_owned(),
                        Search::new(collection_id, condition, join),
                    );
                    return last_xml;
                }
            }
        }
        return xml;
    }

    async fn make_conditions<'a>(
        &mut self,
        vars: &Vars,
        xml: &'a [u8],
    ) -> (&'a [u8], Vec<Condition>, HashMap<String, Join>) {
        let (last_xml, mut conditions, join) = self.condition_loop(xml).await;

        if let Some(activity) = vars.get("activity") {
            conditions.push(Condition::Activity(if activity.to_str() == "inactive" {
                Activity::Inactive
            } else {
                Activity::Active
            }));
        }
        if let Some(term) = vars.get("term") {
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
        (last_xml, conditions, join)
    }

    #[async_recursion(?Send)]
    async fn condition_loop<'a>(
        &mut self,
        xml: &'a [u8],
    ) -> (&'a [u8], Vec<Condition>, HashMap<String, Join>) {
        let mut join = HashMap::new();
        let mut result_conditions = Vec::new();
        let mut xml = xml;
        let mut scanner = Scanner::new();

        let mut futs = vec![];

        while let Some(state) = scanner.scan(xml) {
            match state {
                State::ScannedStartTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::StartTag::from(token_bytes);
                    let name = token.name();
                    if let None = name.namespace_prefix() {
                        match name.local().as_bytes() {
                            b"narrow" => {
                                let (inner_xml, outer_end) = xml_util::inner(xml);
                                xml = &xml[outer_end..];
                                if let Ok(inner_xml) = self.parse(inner_xml).await.as_mut() {
                                    let (_, cond, _) = self.condition_loop(&inner_xml).await;
                                    result_conditions.push(Condition::Narrow(cond));
                                }
                            }
                            b"wide" => {
                                let (inner_xml, outer_end) = xml_util::inner(xml);
                                xml = &xml[outer_end..];
                                if let Ok(inner_xml) = self.parse(inner_xml).await {
                                    let (_, cond, _) = self.condition_loop(&inner_xml).await;
                                    result_conditions.push(Condition::Wide(cond));
                                }
                            }
                            b"join" => {
                                let vars = self.vars_from_attibutes(token.attributes()).await;
                                xml = self.join(xml, &vars, &mut join).await;
                            }
                            _ => {}
                        }
                    }
                }
                State::ScannedEmptyElementTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::EmptyElementTag::from(token_bytes);
                    let attributes = self.vars_from_attibutes(token.attributes()).await;
                    let name = token.name();
                    match name.local().as_bytes() {
                        b"row" => futs.push(Self::condition_row(attributes).boxed_local()),
                        b"field" => futs.push(Self::condition_field(attributes).boxed_local()),
                        b"uuid" => futs.push(Self::condition_uuid(attributes).boxed_local()),
                        b"depend" => {
                            if let Some(c) = self.condition_depend(attributes).await {
                                result_conditions.push(c);
                            }
                        }
                        _ => {}
                    }
                }
                State::ScannedEndTag(pos) => {
                    let token = token::EndTag::from(&xml[..pos]);
                    xml = &xml[pos..];
                    match token.name().as_bytes() {
                        b"wd:search" | b"narrow" | b"wide" => {
                            break;
                        }
                        _ => {}
                    }
                }
                State::ScannedCharacters(pos)
                | State::ScannedCdata(pos)
                | State::ScannedComment(pos)
                | State::ScannedDeclaration(pos)
                | State::ScannedProcessingInstruction(pos) => {
                    xml = &xml[pos..];
                }
                _ => {}
            }
        }
        result_conditions.extend(futures::future::join_all(futs).await.into_iter().flatten());
        (xml, result_conditions, join)
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
