use std::{
    collections::HashMap,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::TimeZone;
use deno_runtime::worker::MainWorker;
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use semilattice_database_session::{search, Activity, CollectionRow, Condition, Depend, Uuid};
use xmlparser::{ElementEnd, Token, Tokenizer};

use crate::xml_util::{self, XmlAttr};

use super::Script;

pub fn search(
    script: &mut Script,
    worker: &mut MainWorker,
    reader: &mut Reader<&[u8]>,
    e: &BytesStart,
    search_map: &mut HashMap<String, (i32, Vec<Condition>)>,
) {
    let attr = xml_util::attr2hash_map(&e);
    let name = crate::attr_parse_or_static_string(worker, &attr, "name");
    let collection_name = crate::attr_parse_or_static_string(worker, &attr, "collection");
    if name != "" && collection_name != "" {
        if let Some(collection_id) = script
            .database
            .clone()
            .read()
            .unwrap()
            .collection_id(&collection_name)
        {
            let condition = make_conditions(script, &attr, reader, worker);
            search_map.insert(name.to_owned(), (collection_id, condition));
            return;
        }
    }
    let _ = reader.read_to_end(quick_xml::name::QName(b"wd:search"));
}
pub fn search_xml_parser(
    script: &mut Script,
    worker: &mut MainWorker,
    tokenizer: &mut Tokenizer,
    attributes: &HashMap<(String, String), String>,
    search_map: &mut HashMap<String, (i32, Vec<Condition>)>,
) {
    let name = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "name");
    let collection_name =
        crate::attr_parse_or_static_string_xml_parser(worker, attributes, "collection");
    if name != "" && collection_name != "" {
        if let Some(collection_id) = script
            .database
            .clone()
            .read()
            .unwrap()
            .collection_id(&collection_name)
        {
            let condition = make_conditions_xml_parser(script, attributes, tokenizer, worker);
            search_map.insert(name.to_owned(), (collection_id, condition));
            return;
        }
    }
    let _ = xml_util::inner_xml_parser("wd", "search", tokenizer);
}
fn make_conditions_xml_parser(
    script: &Script,
    attributes: &HashMap<(String, String), String>,
    tokenizer: &mut Tokenizer,
    worker: &mut MainWorker,
) -> Vec<Condition> {
    let mut conditions = condition_loop_xml_parser(script, tokenizer, worker);

    if let Some(activity) = attributes.get(&("".to_owned(), "activity".to_owned())) {
        if activity == "inactive" {
            conditions.push(Condition::Activity(Activity::Inactive));
        } else if activity == "active" {
            conditions.push(Condition::Activity(Activity::Active));
        }
    }
    if let Some(term) = attributes.get(&("".to_owned(), "term".to_owned())) {
        if term != "all" {
            let term: Vec<&str> = term.split("@").collect();
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
    conditions
}

fn make_conditions(
    script: &Script,
    attr: &XmlAttr,
    reader: &mut Reader<&[u8]>,
    worker: &mut MainWorker,
) -> Vec<Condition> {
    let mut conditions = condition_loop(script, reader, worker);

    if let Some(activity) = attr.get("activity") {
        if let Ok(activity) = std::str::from_utf8(activity) {
            if activity == "inactive" {
                conditions.push(Condition::Activity(Activity::Inactive));
            } else if activity == "active" {
                conditions.push(Condition::Activity(Activity::Active));
            }
        }
    }
    if let Some(term) = attr.get("term") {
        if let Ok(term) = std::str::from_utf8(term) {
            if term != "all" {
                let term: Vec<&str> = term.split("@").collect();
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
    }
    conditions
}

fn condition_depend(
    script: &Script,
    worker: &mut MainWorker,
    e: &quick_xml::events::BytesStart,
) -> Option<Condition> {
    let attr = xml_util::attr2hash_map(e);
    let row = crate::attr_parse_or_static_string(worker, &attr, "row");
    let collection_name = crate::attr_parse_or_static_string(worker, &attr, "collection");

    if row != "" && collection_name != "" {
        if let (Ok(row), Some(collection_id)) = (
            row.parse::<i64>(),
            script
                .database
                .clone()
                .read()
                .unwrap()
                .collection_id(&collection_name),
        ) {
            let in_session = row < 0;

            let key = crate::attr_parse_or_static_string(worker, &attr, "key");
            return Some(Condition::Depend(Depend::new(
                &key,
                if in_session {
                    CollectionRow::new(-collection_id, (-row) as u32)
                } else {
                    CollectionRow::new(collection_id, row as u32)
                },
            )));
        }
    }
    None
}

fn condition_depend_xml_parser(
    script: &Script,
    worker: &mut MainWorker,
    attributes: &HashMap<(String, String), String>,
) -> Option<Condition> {
    let row = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "row");
    let collection_name =
        crate::attr_parse_or_static_string_xml_parser(worker, attributes, "collection");

    if row != "" && collection_name != "" {
        if let (Ok(row), Some(collection_id)) = (
            row.parse::<i64>(),
            script
                .database
                .clone()
                .read()
                .unwrap()
                .collection_id(&collection_name),
        ) {
            let in_session = row < 0;

            let key = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "key");
            return Some(Condition::Depend(Depend::new(
                &key,
                if in_session {
                    CollectionRow::new(-collection_id, (-row) as u32)
                } else {
                    CollectionRow::new(collection_id, row as u32)
                },
            )));
        }
    }
    None
}

fn condition_loop_xml_parser(
    script: &Script,
    tokenizer: &mut Tokenizer,
    worker: &mut MainWorker,
) -> Vec<Condition> {
    let mut conditions = Vec::new();
    while let Some(Ok(token)) = tokenizer.next() {
        match token {
            Token::ElementStart { prefix, local, .. } => {
                let (attributes_str, attributes) = xml_util::attributes(tokenizer);
                if prefix.as_str() == "" {
                    match local.as_str() {
                        "field" => {
                            if let Some(c) = condition_field_xml_parser(&attributes, worker) {
                                conditions.push(c);
                            }
                        }
                        "row" => {
                            if let Some(c) = condition_row_xml_parser(&attributes, worker) {
                                conditions.push(c);
                            }
                        }
                        "uuid" => {
                            if let Some(c) = condition_uuid_xml_parser(&attributes, worker) {
                                conditions.push(c);
                            }
                        }
                        "narrow" => {
                            conditions.push(Condition::Narrow(condition_loop_xml_parser(
                                script, tokenizer, worker,
                            )));
                        }
                        "wide" => {
                            conditions.push(Condition::Wide(condition_loop_xml_parser(
                                script, tokenizer, worker,
                            )));
                        }
                        "depend" => {
                            if let Some(c) =
                                condition_depend_xml_parser(script, worker, &attributes)
                            {
                                conditions.push(c);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Token::ElementEnd {
                end: ElementEnd::Close(prefix, local),
                ..
            } => match (prefix.as_str(), local.as_str()) {
                ("wd", "search") | ("", "narrow") | ("", "wide") => {
                    break;
                }
                _ => {}
            },
            _ => {}
        }
    }
    conditions
}

fn condition_loop(
    script: &Script,
    reader: &mut Reader<&[u8]>,
    worker: &mut MainWorker,
) -> Vec<Condition> {
    let mut conditions = Vec::new();
    loop {
        if let Ok(next) = reader.read_event() {
            match next {
                Event::Start(ref e) => match e.name().as_ref() {
                    b"field" => {
                        if let Some(c) = condition_field(xml_util::attr2hash_map(&e), worker) {
                            conditions.push(c);
                        }
                    }
                    b"row" => {
                        if let Some(c) = condition_row(xml_util::attr2hash_map(&e), worker) {
                            conditions.push(c);
                        }
                    }
                    b"uuid" => {
                        if let Some(c) = condition_uuid(xml_util::attr2hash_map(&e), worker) {
                            conditions.push(c);
                        }
                    }
                    b"narrow" => {
                        conditions.push(Condition::Narrow(condition_loop(script, reader, worker)));
                    }
                    b"wide" => {
                        conditions.push(Condition::Wide(condition_loop(script, reader, worker)));
                    }
                    b"depend" => {
                        if let Some(c) = condition_depend(script, worker, e) {
                            conditions.push(c);
                        }
                    }
                    _ => {}
                },
                Event::Empty(ref e) => match e.name().as_ref() {
                    b"field" => {
                        if let Some(c) = condition_field(xml_util::attr2hash_map(e), worker) {
                            conditions.push(c);
                        }
                    }
                    b"row" => {
                        if let Some(c) = condition_row(xml_util::attr2hash_map(e), worker) {
                            conditions.push(c);
                        }
                    }
                    b"uuid" => {
                        if let Some(c) = condition_uuid(xml_util::attr2hash_map(e), worker) {
                            conditions.push(c);
                        }
                    }
                    b"depend" => {
                        if let Some(c) = condition_depend(script, worker, e) {
                            conditions.push(c);
                        }
                    }
                    _ => {}
                },
                Event::End(e) => match e.name().as_ref() {
                    b"wd:search" | b"narrow" | b"wide" => {
                        break;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
    conditions
}

fn condition_row<'a>(attr: XmlAttr, worker: &mut MainWorker) -> Option<Condition> {
    let method = crate::attr_parse_or_static(worker, &attr, "method");
    let value = crate::attr_parse_or_static_string(worker, &attr, "value");
    if value != "" {
        match &*method {
            b"in" => {
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
            b"min" => {
                if let Ok(v) = value.parse::<isize>() {
                    return Some(Condition::Row(search::Number::Min(v)));
                }
            }
            b"max" => {
                if let Ok(v) = value.parse::<isize>() {
                    return Some(Condition::Row(search::Number::Max(v)));
                }
            }
            b"range" => {
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
    None
}
fn condition_uuid<'a>(attr: XmlAttr, worker: &mut MainWorker) -> Option<Condition> {
    let value = crate::attr_parse_or_static_string(worker, &attr, "value");
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
    None
}
fn condition_field<'a>(attr: XmlAttr, worker: &mut MainWorker) -> Option<Condition> {
    let name = crate::attr_parse_or_static_string(worker, &attr, "name");
    let method = crate::attr_parse_or_static_string(worker, &attr, "method");
    let value = crate::attr_parse_or_static_string(worker, &attr, "value");

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
            _ => None,
        } {
            return Some(Condition::Field(name.to_string(), method));
        }
    }
    None
}

fn condition_row_xml_parser<'a>(
    attributes: &HashMap<(String, String), String>,
    worker: &mut MainWorker,
) -> Option<Condition> {
    let method = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "method");
    let value = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "value");
    if value != "" {
        match method.as_str() {
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
    None
}
fn condition_uuid_xml_parser<'a>(
    attributes: &HashMap<(String, String), String>,
    worker: &mut MainWorker,
) -> Option<Condition> {
    let value = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "value");
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
    None
}
fn condition_field_xml_parser<'a>(
    attributes: &HashMap<(String, String), String>,
    worker: &mut MainWorker,
) -> Option<Condition> {
    let name = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "name");
    let method = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "method");
    let value = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "value");

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
            _ => None,
        } {
            return Some(Condition::Field(name.to_string(), method));
        }
    }
    None
}
