use std::{
    collections::HashMap,
    io::BufReader,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::TimeZone;
use deno_runtime::worker::MainWorker;
use maybe_xml::token::owned::Token;
use semilattice_database_session::{search, Activity, CollectionRow, Condition, Depend, Uuid};

use crate::xml_util;

use super::Script;

pub fn search(
    script: &mut Script,
    worker: &mut MainWorker,
    tokenizer: &mut maybe_xml::eval::bufread::IntoIter<BufReader<&[u8]>>,
    tag_name: &maybe_xml::token::prop::TagName,
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    search_map: &mut HashMap<String, (i32, Vec<Condition>)>,
) {
    let name = crate::attr_parse_or_static_string(worker, attributes, b"name");
    let collection_name = crate::attr_parse_or_static_string(worker, attributes, b"collection");
    if name != "" && collection_name != "" {
        if let Some(collection_id) = script
            .database
            .clone()
            .read()
            .unwrap()
            .collection_id(&collection_name)
        {
            let condition = make_conditions(script, attributes, tokenizer, worker);
            search_map.insert(name.to_owned(), (collection_id, condition));
            return;
        }
    }
    let _ = xml_util::inner(tag_name, tokenizer);
}
fn make_conditions(
    script: &Script,
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    tokenizer: &mut maybe_xml::eval::bufread::IntoIter<BufReader<&[u8]>>,
    worker: &mut MainWorker,
) -> Vec<Condition> {
    let mut conditions = condition_loop(script, tokenizer, worker);

    if let Some((None, Some(activity))) = attributes.get(b"activity".as_slice()) {
        if activity == b"inactive" {
            conditions.push(Condition::Activity(Activity::Inactive));
        } else if activity == b"active" {
            conditions.push(Condition::Activity(Activity::Active));
        }
    }
    if let Some((None, Some(term))) = attributes.get(b"term".as_slice()) {
        if term != b"all" {
            let term: Vec<&[u8]> = term.split(|c| *c == b'@').collect();
            if term.len() == 2 {
                conditions.push(Condition::Term(
                    chrono::Local
                        .datetime_from_str(
                            std::str::from_utf8(term[1]).unwrap(),
                            "%Y-%m-%d %H:%M:%S",
                        )
                        .map_or(
                            search::Term::In(
                                SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs(),
                            ),
                            |t| match term[0] {
                                b"in" => search::Term::In(t.timestamp() as u64),
                                b"future" => search::Term::Future(t.timestamp() as u64),
                                b"past" => search::Term::Past(t.timestamp() as u64),
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

fn condition_depend(
    script: &Script,
    worker: &mut MainWorker,
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
) -> Option<Condition> {
    let row = crate::attr_parse_or_static_string(worker, attributes, b"row");
    let collection_name = crate::attr_parse_or_static_string(worker, attributes, b"collection");

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

            let key = crate::attr_parse_or_static_string(worker, attributes, b"key");
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

fn condition_loop(
    script: &Script,
    tokenizer: &mut maybe_xml::eval::bufread::IntoIter<BufReader<&[u8]>>,
    worker: &mut MainWorker,
) -> Vec<Condition> {
    let mut conditions = Vec::new();
    while let Some(token) = tokenizer.next() {
        match token {
            Token::StartTag(tag) => {
                let name = tag.name();
                if let None = name.namespace_prefix() {
                    match name.local().as_bytes() {
                        b"narrow" => {
                            conditions
                                .push(Condition::Narrow(condition_loop(script, tokenizer, worker)));
                        }
                        b"wide" => {
                            conditions
                                .push(Condition::Wide(condition_loop(script, tokenizer, worker)));
                        }
                        _ => {}
                    }
                }
            }
            Token::EmptyElementTag(tag) => {
                let name = tag.name();
                if let None = name.namespace_prefix() {
                    match name.local().as_bytes() {
                        b"row" => {
                            if let Some(c) =
                                condition_row(&crate::attr2map(&tag.attributes()), worker)
                            {
                                conditions.push(c);
                            }
                        }
                        b"field" => {
                            if let Some(c) =
                                condition_field(&crate::attr2map(&tag.attributes()), worker)
                            {
                                conditions.push(c);
                            }
                        }
                        b"uuid" => {
                            if let Some(c) =
                                condition_uuid(&crate::attr2map(&tag.attributes()), worker)
                            {
                                conditions.push(c);
                            }
                        }
                        b"depend" => {
                            if let Some(c) = condition_depend(
                                script,
                                worker,
                                &crate::attr2map(&tag.attributes()),
                            ) {
                                conditions.push(c);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Token::EndTag(tag) => {
                let name = tag.name();
                match (
                    if let Some(prefix) = name.namespace_prefix() {
                        prefix.to_vec()
                    } else {
                        b"".to_vec()
                    }
                    .as_slice(),
                    name.local().as_bytes(),
                ) {
                    (b"wd", b"search") | (b"", b"narrow") | (b"", b"wide") => {
                        break;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    conditions
}

fn condition_row<'a>(
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    worker: &mut MainWorker,
) -> Option<Condition> {
    let method = crate::attr_parse_or_static_string(worker, attributes, b"method");
    let value = crate::attr_parse_or_static_string(worker, attributes, b"value");
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
fn condition_uuid<'a>(
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    worker: &mut MainWorker,
) -> Option<Condition> {
    let value = crate::attr_parse_or_static_string(worker, attributes, b"value");
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
fn condition_field<'a>(
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    worker: &mut MainWorker,
) -> Option<Condition> {
    let name = crate::attr_parse_or_static_string(worker, attributes, b"name");
    let method = crate::attr_parse_or_static_string(worker, attributes, b"method");
    let value = crate::attr_parse_or_static_string(worker, attributes, b"value");

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
