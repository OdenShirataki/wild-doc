use std::{
    collections::HashMap,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::TimeZone;
use maybe_xml::{
    scanner::{Scanner, State},
    token,
};
use semilattice_database_session::{search, Activity, CollectionRow, Condition, Depend, Uuid};

use super::{AttributeMap, Parser};

impl Parser {
    pub(crate) fn search<'a>(
        &mut self,
        xml: &'a [u8],
        attributes: &AttributeMap,
        search_map: &mut HashMap<String, (i32, Vec<Condition>)>,
    ) -> &'a [u8] {
        if let (Some(Some(name)), Some(Some(collection_name))) = (
            attributes.get(b"name".as_ref()),
            attributes.get(b"collection".as_ref()),
        ) {
            let name = name.to_str();
            let collection_name = collection_name.to_str();
            if name != "" && collection_name != "" {
                if let Some(collection_id) = self
                    .database
                    .clone()
                    .read()
                    .unwrap()
                    .collection_id(collection_name.as_ref())
                {
                    let (last_xml, condition) = self.make_conditions(attributes, xml);
                    search_map.insert(name.into_owned(), (collection_id, condition));
                    return last_xml;
                }
                if let Some(Some(value)) =
                    attributes.get(b"create_collection_if_not_exists".as_ref())
                {
                    if value.to_str() == "true" {
                        if let Ok(collection_id) = self
                            .database
                            .clone()
                            .write()
                            .unwrap()
                            .collection_id_or_create(collection_name.as_ref())
                        {
                            let (last_xml, condition) = self.make_conditions(attributes, xml);
                            search_map.insert(name.into_owned(), (collection_id, condition));
                            return last_xml;
                        }
                    }
                }
            }
        }
        return xml;
    }
    fn make_conditions<'a>(
        &mut self,
        attributes: &AttributeMap,
        xml: &'a [u8],
    ) -> (&'a [u8], Vec<Condition>) {
        let (last_xml, mut conditions) = self.condition_loop(xml);

        if let Some(Some(activity)) = attributes.get(b"activity".as_ref()) {
            let activity = activity.to_str();
            if activity == "inactive" {
                conditions.push(Condition::Activity(Activity::Inactive));
            } else if activity == "active" {
                conditions.push(Condition::Activity(Activity::Active));
            }
        }
        if let Some(Some(term)) = attributes.get(b"term".as_ref()) {
            let term = term.to_str();
            if term != "all" {
                let term: Vec<&str> = term.split('@').collect();
                if term.len() == 2 {
                    conditions.push(Condition::Term(
                        chrono::Local
                            .datetime_from_str(term[1], "%Y-%m-%d %H:%M:%S")
                            .map_or(
                                search::Term::In(
                                    SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap()
                                        .as_secs(),
                                ),
                                |t| match term[0] {
                                    "in" => search::Term::In(t.timestamp() as u64),
                                    "future" => search::Term::Future(t.timestamp() as u64),
                                    "past" => search::Term::Past(t.timestamp() as u64),
                                    _ => search::Term::In(
                                        SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap()
                                            .as_secs(),
                                    ),
                                },
                            ),
                    ));
                } else {
                    conditions.push(Condition::Term(search::Term::In(
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    )));
                }
            }
        }
        (last_xml, conditions)
    }

    fn condition_loop<'a>(&mut self, xml: &'a [u8]) -> (&'a [u8], Vec<Condition>) {
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
                                let (last_xml, cond) = self.condition_loop(xml);
                                result_conditions.push(Condition::Narrow(cond));
                                xml = last_xml;
                            }
                            b"wide" => {
                                let (last_xml, cond) = self.condition_loop(xml);
                                result_conditions.push(Condition::Wide(cond));
                                xml = last_xml;
                            }
                            _ => {}
                        }
                    }
                }
                State::ScannedEmptyElementTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::EmptyElementTag::from(token_bytes);
                    let attributes = self.parse_attibutes(token.attributes());
                    let name = token.name();
                    match name.local().as_bytes() {
                        b"row" => {
                            if let Some(c) = self.condition_row(&attributes) {
                                result_conditions.push(c);
                            }
                        }
                        b"field" => {
                            if let Some(c) = self.condition_field(&attributes) {
                                result_conditions.push(c);
                            }
                        }
                        b"uuid" => {
                            if let Some(c) = self.condition_uuid(&attributes) {
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
        (xml, result_conditions)
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
                    return Some(Condition::Depend(Depend::new(
                        if let Some(Some(akey)) = attributes.get(b"key".as_ref()) {
                            akey.to_str()
                        } else {
                            "".into()
                        },
                        if row < 0 {
                            CollectionRow::new(-collection_id, (-row) as u32)
                        } else {
                            CollectionRow::new(collection_id, row as u32)
                        },
                    )));
                }
            }
        }
        None
    }
    fn condition_row<'a>(&mut self, attributes: &AttributeMap) -> Option<Condition> {
        if let (Some(Some(method)), Some(Some(value))) = (
            attributes.get(b"method".as_ref()),
            attributes.get(b"value".as_ref()),
        ) {
            let value = value.to_str();
            if value != "" {
                match method.to_str().as_ref() {
                    "in" => {
                        let mut v = Vec::<isize>::new();
                        for s in value.split(',') {
                            if let Ok(i) = s.parse::<isize>() {
                                v.push(i);
                            }
                        }
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

    fn condition_uuid<'a>(&mut self, attributes: &AttributeMap) -> Option<Condition> {
        if let Some(Some(value)) = attributes.get(b"value".as_ref()) {
            let value = value.to_str();
            if value != "" {
                let mut v = Vec::<u128>::new();
                for s in value.split(',') {
                    if let Ok(uuid) = Uuid::from_str(&s) {
                        v.push(uuid.as_u128());
                    }
                }
                if v.len() > 0 {
                    return Some(Condition::Uuid(v));
                }
            }
        }
        None
    }

    fn condition_field<'a>(&mut self, attributes: &AttributeMap) -> Option<Condition> {
        if let (Some(Some(name)), Some(Some(method)), Some(Some(value))) = (
            attributes.get(b"name".as_ref()),
            attributes.get(b"method".as_ref()),
            attributes.get(b"value".as_ref()),
        ) {
            let name = name.to_str();
            let method = method.to_str();
            let value = value.to_str();
            if name != "" && method != "" && value != "" {
                let method_pair: Vec<&str> = method.split('!').collect();
                let len = method_pair.len();
                let i = len - 1;

                if let Some(method) = match method_pair[i] {
                    "match" => Some(search::Field::Match(value.as_bytes().to_vec())),
                    "min" => Some(search::Field::Min(value.as_bytes().to_vec())),
                    "max" => Some(search::Field::Max(value.as_bytes().to_vec())),
                    "partial" => Some(search::Field::Partial(value.to_string())),
                    "forward" => Some(search::Field::Forward(value.to_string())),
                    "backward" => Some(search::Field::Backward(value.to_string())),
                    "range" => {
                        let s: Vec<&str> = value.split("..").collect();
                        if s.len() == 2 {
                            Some(search::Field::Range(
                                s[0].as_bytes().to_vec(),
                                s[1].as_bytes().to_vec(),
                            ))
                        } else {
                            None
                        }
                    }
                    "value_forward" => Some(search::Field::ValueForward(value.to_string())),
                    "value_backward" => Some(search::Field::ValueBackward(value.to_string())),
                    "value_partial" => Some(search::Field::ValuePartial(value.to_string())),
                    _ => None,
                } {
                    return Some(Condition::Field(name.to_string(), method));
                }
            }
        }
        None
    }
}
