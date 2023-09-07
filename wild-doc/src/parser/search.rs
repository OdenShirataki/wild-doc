mod join;

use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, RwLock},
};

use chrono::TimeZone;
use maybe_xml::{
    scanner::{Scanner, State},
    token,
};
use semilattice_database_session::{
    search::{self, Join, Search},
    Activity, CollectionRow, Condition, Uuid,
};

use crate::xml_util;

use super::{AttributeMap, Parser};

impl Parser {
    fn collection_id(&mut self, attributes: &AttributeMap) -> Option<i32> {
        if let Some(Some(collection_name)) = attributes.get(b"collection".as_ref()) {
            let collection_name = collection_name.to_str();
            self.database
                .clone()
                .read()
                .unwrap()
                .collection_id(collection_name.as_ref())
                .map_or_else(
                    || {
                        (collection_name != "")
                            .then(|| {
                                if let Some(Some(value)) =
                                    attributes.get(b"create_collection_if_not_exists".as_ref())
                                {
                                    (value.to_str() == "true").then(|| {
                                        self.database
                                            .clone()
                                            .write()
                                            .unwrap()
                                            .collection_id_or_create(collection_name.as_ref())
                                    })
                                } else {
                                    None
                                }
                            })
                            .and_then(|v| v)
                    },
                    |collection_id| Some(collection_id),
                )
        } else {
            None
        }
    }

    pub(crate) fn search<'a>(
        &mut self,
        xml: &'a [u8],
        attributes: &AttributeMap,
        search_map: &mut HashMap<String, Arc<RwLock<Search>>>,
    ) -> &'a [u8] {
        if let Some(Some(name)) = attributes.get(b"name".as_ref()) {
            let name = name.to_str();
            if name != "" {
                if let Some(collection_id) = self.collection_id(attributes) {
                    let (last_xml, condition, join) = self.make_conditions(attributes, xml);
                    search_map.insert(
                        name.into_owned(),
                        Arc::new(RwLock::new(Search::new(collection_id, condition, join))),
                    );
                    return last_xml;
                }
            }
        }
        return xml;
    }

    fn make_conditions<'a>(
        &mut self,
        attributes: &AttributeMap,
        xml: &'a [u8],
    ) -> (&'a [u8], Vec<Condition>, HashMap<String, Join>) {
        let (last_xml, mut conditions, join) = self.condition_loop(xml);

        if let Some(Some(activity)) = attributes.get(b"activity".as_ref()) {
            let activity = activity.to_str();
            conditions.push(Condition::Activity(if activity == "inactive" {
                Activity::Inactive
            } else {
                Activity::Active
            }));
        }
        if let Some(Some(term)) = attributes.get(b"term".as_ref()) {
            let term = term.to_str();
            if term != "all" {
                let term: Vec<&str> = term.split('@').collect();
                conditions.push(Condition::Term(if term.len() == 2 {
                    chrono::Local
                        .datetime_from_str(term[1], "%Y-%m-%d %H:%M:%S")
                        .map_or_else(
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

    fn condition_loop<'a>(
        &mut self,
        xml: &'a [u8],
    ) -> (&'a [u8], Vec<Condition>, HashMap<String, Join>) {
        let mut join = HashMap::new();
        let mut result_conditions = Vec::new();
        let mut xml = xml;
        let mut scanner = Scanner::new();
        while let Some(state) = scanner.scan(xml) {
            match state {
                State::ScannedStartTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::StartTag::from(token_bytes);
                    let name = token.name();
                    if let None = name.namespace_prefix() {
                        match name.local().as_bytes() {
                            b"narrow" => {
                                let (inner_xml, outer_end) = xml_util::inner(xml);
                                xml = &xml[outer_end..];
                                if let Ok(inner_xml) = self.parse(inner_xml) {
                                    let (_, cond, _) = self.condition_loop(&inner_xml);
                                    result_conditions.push(Condition::Narrow(cond));
                                }
                            }
                            b"wide" => {
                                let (inner_xml, outer_end) = xml_util::inner(xml);
                                xml = &xml[outer_end..];
                                if let Ok(inner_xml) = self.parse(inner_xml) {
                                    let (_, cond, _) = self.condition_loop(&inner_xml);
                                    result_conditions.push(Condition::Wide(cond));
                                }
                            }
                            b"join" => {
                                let attributes = self.parse_attibutes(&token.attributes());
                                xml = self.join(xml, &attributes, &mut join);
                            }
                            _ => {}
                        }
                    }
                }
                State::ScannedEmptyElementTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::EmptyElementTag::from(token_bytes);
                    let attributes = self.parse_attibutes(&token.attributes());
                    let name = token.name();
                    match name.local().as_bytes() {
                        b"row" => {
                            if let Some(c) = Self::condition_row(&attributes) {
                                result_conditions.push(c);
                            }
                        }
                        b"field" => {
                            if let Some(c) = Self::condition_field(&attributes) {
                                result_conditions.push(c);
                            }
                        }
                        b"uuid" => {
                            if let Some(c) = Self::condition_uuid(&attributes) {
                                result_conditions.push(c);
                            }
                        }
                        b"depend" => {
                            if let Some(c) = self.condition_depend(&attributes) {
                                result_conditions.push(c);
                            }
                        }
                        _ => {}
                    }
                }
                State::ScannedEndTag(pos) => {
                    let token = token::borrowed::EndTag::from(&xml[..pos]);
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
        (xml, result_conditions, join)
    }

    fn condition_depend(&mut self, attributes: &AttributeMap) -> Option<Condition> {
        if let (Some(Some(row)), Some(Some(collection_name))) = (
            attributes.get(b"row".as_ref()),
            attributes.get(b"collection".as_ref()),
        ) {
            let row = row.to_str();
            let collection_name = collection_name.to_str();
            if row != "" && collection_name != "" {
                if let (Ok(row), Some(collection_id)) = (
                    row.parse::<i64>(),
                    self.database
                        .clone()
                        .read()
                        .unwrap()
                        .collection_id(&collection_name),
                ) {
                    return Some(Condition::Depend(
                        attributes
                            .get(b"key".as_ref())
                            .and_then(|v| v.as_ref())
                            .map(|v| v.to_str().into_owned()),
                        if row < 0 {
                            CollectionRow::new(-collection_id, (-row) as u32)
                        } else {
                            CollectionRow::new(collection_id, row as u32)
                        },
                    ));
                }
            }
        }
        None
    }
    fn condition_row(attributes: &AttributeMap) -> Option<Condition> {
        if let (Some(Some(method)), Some(Some(value))) = (
            attributes.get(b"method".as_ref()),
            attributes.get(b"value".as_ref()),
        ) {
            let value = value.to_str();
            if value != "" {
                match method.to_str().as_ref() {
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
                        let s: Vec<&str> = value.split("..").collect();
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

    fn condition_uuid(attributes: &AttributeMap) -> Option<Condition> {
        if let Some(Some(value)) = attributes.get(b"value".as_ref()) {
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

    fn condition_field(attributes: &AttributeMap) -> Option<Condition> {
        if let (Some(Some(name)), Some(Some(method)), Some(Some(value))) = (
            attributes.get(b"name".as_ref()),
            attributes.get(b"method".as_ref()),
            attributes.get(b"value".as_ref()),
        ) {
            let name = name.to_str();
            let method = method.to_str();
            let value = value.to_str();
            (name != "" && method != "" && value != "")
                .then(|| {
                    let method_pair: Vec<&str> = method.split('!').collect();
                    let len = method_pair.len();
                    let i = len - 1;
                    match method_pair[i] {
                        "match" => Some(search::Field::Match(value.as_bytes().to_vec())),
                        "min" => Some(search::Field::Min(value.as_bytes().to_vec())),
                        "max" => Some(search::Field::Max(value.as_bytes().to_vec())),
                        "partial" => Some(search::Field::Partial(Arc::new(value.to_string()))),
                        "forward" => Some(search::Field::Forward(Arc::new(value.to_string()))),
                        "backward" => Some(search::Field::Backward(Arc::new(value.to_string()))),
                        "range" => {
                            let s: Vec<&str> = value.split("..").collect();
                            (s.len() == 2).then(|| {
                                search::Field::Range(
                                    s[0].as_bytes().to_vec(),
                                    s[1].as_bytes().to_vec(),
                                )
                            })
                        }
                        "value_forward" => {
                            Some(search::Field::ValueForward(Arc::new(value.to_string())))
                        }
                        "value_backward" => {
                            Some(search::Field::ValueBackward(Arc::new(value.to_string())))
                        }
                        "value_partial" => {
                            Some(search::Field::ValuePartial(Arc::new(value.to_string())))
                        }
                        _ => None,
                    }
                    .map(|method| Condition::Field(name.to_string(), method))
                })
                .and_then(|v| v)
        } else {
            None
        }
    }
}
