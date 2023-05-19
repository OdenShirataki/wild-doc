use chrono::TimeZone;
use deno_runtime::{deno_core::serde_json, worker::MainWorker};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use semilattice_database_session::{
    Activity, CollectionRow, Depends, KeyValue, Pend, Record, Term,
};
use std::{collections::HashMap, error, fmt};
use xmlparser::{ElementEnd, Token, Tokenizer};

use crate::{
    anyhow::{anyhow, Result},
    xml_util,
};

use super::Script;

pub fn update_xml_parser<T: crate::IncludeAdaptor>(
    script: &mut Script,
    worker: &mut MainWorker,
    tokeninzer: &mut xmlparser::Tokenizer,
    attributes: &HashMap<(String, String), String>,
    include_adaptor: &mut T,
) -> Result<()> {
    let inner_xml =
        script.parse_xml_parser(worker, tokeninzer, ("wd", "update"), include_adaptor)?;
    let mut inner_tokeninzer = xmlparser::Tokenizer::from(std::str::from_utf8(&inner_xml).unwrap());
    let updates = make_update_struct_xml_parser(script, &mut inner_tokeninzer, worker)?;
    if let Some((ref mut session, _)) = script.sessions.last_mut() {
        let session_rows = script
            .database
            .clone()
            .read()
            .unwrap()
            .update(session, updates)?;
        let commit_rows =
            if crate::attr_parse_or_static_xml_parser(worker, attributes, "commit") == b"1" {
                script.database.clone().write().unwrap().commit(session)?
            } else {
                vec![]
            };
        let src =
            crate::attr_parse_or_static_string_xml_parser(worker, attributes, "result_callback");
        if src.len() > 0 {
            if let (Ok(json_commit_rows), Ok(json_session_rows)) = (
                serde_json::to_string(&commit_rows),
                serde_json::to_string(&session_rows),
            ) {
                let _ = worker.execute_script(
                    "commit",
                    ("{const update_result={commit_rows:".to_owned()
                        + json_commit_rows.as_str()
                        + ",session_rows:"
                        + json_session_rows.as_str()
                        + "};"
                        + src.as_str()
                        + "}")
                        .into(),
                );
            }
        }
    }
    Ok(())
}
pub fn update<T: crate::IncludeAdaptor>(
    script: &mut Script,
    worker: &mut MainWorker,
    reader: &mut Reader<&[u8]>,
    e: &BytesStart,
    include_adaptor: &mut T,
) -> Result<()> {
    let inner_xml = script.parse(worker, reader, b"wd:update", include_adaptor)?;
    let mut inner_reader = Reader::from_str(std::str::from_utf8(&inner_xml).unwrap());
    inner_reader.check_end_names(false);
    let updates = make_update_struct(script, &mut inner_reader, worker)?;
    if let Some((ref mut session, _)) = script.sessions.last_mut() {
        let session_rows = script
            .database
            .clone()
            .read()
            .unwrap()
            .update(session, updates)?;
        let commit_rows = if crate::attr_parse_or_static(
            worker,
            &xml_util::attr2hash_map(&e),
            "commit",
        ) == b"1"
        {
            script.database.clone().write().unwrap().commit(session)?
        } else {
            vec![]
        };
        let src = crate::attr_parse_or_static_string(
            worker,
            &xml_util::attr2hash_map(&e),
            "result_callback",
        );
        if src.len() > 0 {
            if let (Ok(json_commit_rows), Ok(json_session_rows)) = (
                serde_json::to_string(&commit_rows),
                serde_json::to_string(&session_rows),
            ) {
                let _ = worker.execute_script(
                    "commit",
                    ("{const update_result={commit_rows:".to_owned()
                        + json_commit_rows.as_str()
                        + ",session_rows:"
                        + json_session_rows.as_str()
                        + "};"
                        + src.as_str()
                        + "}")
                        .into(),
                );
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct DependError;
impl fmt::Display for DependError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid row to depend")
    }
}
impl error::Error for DependError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}
fn depend(
    script: &mut Script,
    e: &BytesStart,
    depends: &mut Vec<(String, CollectionRow)>,
    worker: &mut MainWorker,
) -> Result<(), DependError> {
    let attr = xml_util::attr2hash_map(&e);

    let key = crate::attr_parse_or_static_string(worker, &attr, "key");
    let collection = crate::attr_parse_or_static_string(worker, &attr, "collection");
    let row = crate::attr_parse_or_static_string(worker, &attr, "row");

    if let (Ok(row), Some(collection_id)) = (
        row.parse::<i64>(),
        script
            .database
            .clone()
            .read()
            .unwrap()
            .collection_id(&collection),
    ) {
        if row == 0 {
            return Err(DependError);
        } else {
            let in_session = row < 0;
            if in_session {
                let mut valid = false;
                if let Some(session) = script.sessions.pop() {
                    if let Some(temporary_collection) =
                        session.0.temporary_collection(collection_id)
                    {
                        if let Some(_) = temporary_collection.get(&row) {
                            valid = true;
                        }
                    }
                    script.sessions.push(session);
                }
                if !valid {
                    return Err(DependError);
                }
            }
            depends.push((
                key.to_owned(),
                if in_session {
                    CollectionRow::new(-collection_id, (-row) as u32)
                } else {
                    CollectionRow::new(collection_id, row as u32)
                },
            ));
        }
        Ok(())
    } else {
        Err(DependError)
    }
}

fn depend_xml_parser(
    script: &mut Script,
    attirbutes: &HashMap<(String, String), String>,
    depends: &mut Vec<(String, CollectionRow)>,
    worker: &mut MainWorker,
) -> Result<(), DependError> {
    let key = crate::attr_parse_or_static_string_xml_parser(worker, attirbutes, "key");
    let collection =
        crate::attr_parse_or_static_string_xml_parser(worker, attirbutes, "collection");
    let row = crate::attr_parse_or_static_string_xml_parser(worker, attirbutes, "row");

    if let (Ok(row), Some(collection_id)) = (
        row.parse::<i64>(),
        script
            .database
            .clone()
            .read()
            .unwrap()
            .collection_id(&collection),
    ) {
        if row == 0 {
            return Err(DependError);
        } else {
            let in_session = row < 0;
            if in_session {
                let mut valid = false;
                if let Some(session) = script.sessions.pop() {
                    if let Some(temporary_collection) =
                        session.0.temporary_collection(collection_id)
                    {
                        if let Some(_) = temporary_collection.get(&row) {
                            valid = true;
                        }
                    }
                    script.sessions.push(session);
                }
                if !valid {
                    return Err(DependError);
                }
            }
            depends.push((
                key.to_owned(),
                if in_session {
                    CollectionRow::new(-collection_id, (-row) as u32)
                } else {
                    CollectionRow::new(collection_id, row as u32)
                },
            ));
        }
        Ok(())
    } else {
        Err(DependError)
    }
}

fn make_update_struct(
    script: &mut Script,
    reader: &mut Reader<&[u8]>,
    worker: &mut MainWorker,
) -> Result<Vec<Record>> {
    let mut updates = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == b"collection" {
                    if let Ok(Some(collection_name)) = e.try_get_attribute("name") {
                        if let Ok(collection_name) = std::str::from_utf8(&collection_name.value) {
                            let collection_id = script
                                .database
                                .clone()
                                .write()
                                .unwrap()
                                .collection_id_or_create(collection_name)
                                .unwrap();

                            let mut pends = Vec::new();
                            let mut depends = Vec::new();
                            let mut fields = HashMap::new();
                            let mut deps = 1;
                            loop {
                                match reader.read_event() {
                                    Ok(Event::Start(ref e)) => {
                                        deps += 1;
                                        let name = e.name();
                                        let name_ref = name.as_ref();
                                        if name_ref == b"field" {
                                            let field_name = crate::attr_parse_or_static(
                                                worker,
                                                &xml_util::attr2hash_map(&e),
                                                "name",
                                            );
                                            if let (Ok(field_name), Ok(cont)) = (
                                                String::from_utf8(field_name),
                                                reader.read_text(name),
                                            ) {
                                                fields.insert(
                                                    field_name,
                                                    cont.replace("&gt;", ">")
                                                        .replace("&lt;", "<")
                                                        .replace("&#039;", "'")
                                                        .replace("&quot;", "\"")
                                                        .replace("&amp;", "&"),
                                                );
                                            }
                                        } else if name_ref == b"pends" {
                                            if let Ok(inner_xml) = reader.read_text(name) {
                                                let mut reader_inner = Reader::from_str(&inner_xml);
                                                reader_inner.check_end_names(false);
                                                let pends_tmp = make_update_struct(
                                                    script,
                                                    &mut reader_inner,
                                                    worker,
                                                )?;
                                                if let Ok(Some(key)) = e.try_get_attribute("key") {
                                                    if let Ok(key) = std::str::from_utf8(&key.value)
                                                    {
                                                        pends.push(Pend::new(key, pends_tmp));
                                                    }
                                                }
                                            }
                                        } else if name_ref == b"depend" {
                                            depend(script, e, &mut depends, worker)?;
                                        }
                                    }
                                    Ok(Event::Empty(ref e)) => {
                                        if e.name().as_ref() == b"depend" {
                                            depend(script, e, &mut depends, worker)?;
                                        }
                                    }
                                    Ok(Event::End(ref e)) => {
                                        deps -= 1;
                                        if deps < 0 {
                                            return Err(anyhow!("invalid XML"));
                                        }
                                        if e.name().as_ref() == b"collection" {
                                            break;
                                        }
                                    }
                                    Ok(Event::Eof) => {
                                        return Err(anyhow!("invalid XML"));
                                    }
                                    _ => {}
                                }
                            }
                            let attr = xml_util::attr2hash_map(&e);
                            let row: i64 = crate::attr_parse_or_static_string(worker, &attr, "row")
                                .parse()
                                .unwrap_or(0);

                            let is_delete = if let Some(v) = attr.get("delete") {
                                if let Ok(v) = std::str::from_utf8(v) {
                                    v == "1"
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                            let (collection_id, row) = if row < 0 {
                                (-collection_id, (-row) as u32)
                            } else {
                                (collection_id, row as u32)
                            };
                            if is_delete {
                                updates.push(Record::Delete { collection_id, row });
                            } else {
                                let activity =
                                    crate::attr_parse_or_static(worker, &attr, "activity");
                                let activity = match &*activity {
                                    b"inactive" => Activity::Inactive,
                                    b"0" => Activity::Inactive,
                                    _ => Activity::Active,
                                };
                                let term_begin =
                                    crate::attr_parse_or_static_string(worker, &attr, "term_begin");
                                let term_begin = if term_begin != "" {
                                    if let Some(t) = chrono::Local
                                        .datetime_from_str(&term_begin, "%Y-%m-%d %H:%M:%S")
                                        .map_or(None, |v| Some(v.timestamp()))
                                    {
                                        Term::Overwrite(t as u64)
                                    } else {
                                        Term::Default
                                    }
                                } else {
                                    Term::Default
                                };
                                let term_end =
                                    crate::attr_parse_or_static_string(worker, &attr, "term_end");
                                let term_end = if term_end != "" {
                                    if let Some(t) = chrono::Local
                                        .datetime_from_str(&term_end, "%Y-%m-%d %H:%M:%S")
                                        .map_or(None, |v| Some(v.timestamp()))
                                    {
                                        Term::Overwrite(t as u64)
                                    } else {
                                        Term::Default
                                    }
                                } else {
                                    Term::Default
                                };

                                let mut f = Vec::new();
                                for (key, value) in fields {
                                    f.push(KeyValue::new(key, value.as_bytes()))
                                }
                                if row == 0 {
                                    updates.push(Record::New {
                                        collection_id,
                                        activity,
                                        term_begin,
                                        term_end,
                                        fields: f,
                                        depends: Depends::Overwrite(depends),
                                        pends,
                                    });
                                } else {
                                    updates.push(Record::Update {
                                        collection_id,
                                        row,
                                        activity,
                                        term_begin,
                                        term_end,
                                        fields: f,
                                        depends: if crate::attr_parse_or_static_string(
                                            worker,
                                            &attr,
                                            "inherit_depend_if_empty",
                                        ) == "true"
                                            && depends.len() == 0
                                        {
                                            Depends::Default
                                        } else {
                                            Depends::Overwrite(depends)
                                        },
                                        pends,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == b"wd:update" {
                    break;
                }
            }
            Ok(Event::Eof) => {
                break;
            }
            _ => {}
        }
    }
    Ok(updates)
}

fn make_update_struct_xml_parser(
    script: &mut Script,
    tokenizer: &mut xmlparser::Tokenizer,
    worker: &mut MainWorker,
) -> Result<Vec<Record>> {
    let mut updates = Vec::new();
    while let Some(Ok(token)) = tokenizer.next() {
        match token {
            Token::ElementStart { prefix, local, .. } => {
                let (attributes_str, attributes) = xml_util::attributes(tokenizer);
                if prefix.as_str() == "" && local.as_str() == "collection" {
                    if let Some(collection_name) =
                        attributes.get(&("".to_owned(), "name".to_owned()))
                    {
                        let collection_id = script
                            .database
                            .clone()
                            .write()
                            .unwrap()
                            .collection_id_or_create(collection_name)
                            .unwrap();

                        let mut pends = Vec::new();
                        let mut depends = Vec::new();
                        let mut fields = HashMap::new();
                        let mut deps = 1;
                        let next = tokenizer.next();
                        while let Some(Ok(token)) = next {
                            match token {
                                Token::ElementStart { prefix, local, .. } => {
                                    let (attributes_str, attributes) =
                                        xml_util::attributes(tokenizer);
                                    deps += 1;
                                    let prefix = prefix.as_str();
                                    let local = local.as_str();
                                    if prefix == "" {
                                        match local {
                                            "field" => {
                                                let field_name =
                                                    crate::attr_parse_or_static_string_xml_parser(
                                                        worker,
                                                        &attributes,
                                                        "name",
                                                    );
                                                let cont = xml_util::inner_xml_parser(
                                                    prefix, local, tokenizer,
                                                );
                                                fields.insert(
                                                    field_name,
                                                    cont.replace("&gt;", ">")
                                                        .replace("&lt;", "<")
                                                        .replace("&#039;", "'")
                                                        .replace("&quot;", "\"")
                                                        .replace("&amp;", "&"),
                                                );
                                            }
                                            "pends" => {
                                                let inner_xml = xml_util::inner_xml_parser(
                                                    prefix, local, tokenizer,
                                                );
                                                let mut tokenizer_inner =
                                                    Tokenizer::from(inner_xml.as_str());
                                                let pends_tmp = make_update_struct_xml_parser(
                                                    script,
                                                    &mut tokenizer_inner,
                                                    worker,
                                                )?;
                                                if let Some(key) = attributes
                                                    .get(&("".to_string(), "key".to_string()))
                                                {
                                                    pends.push(Pend::new(key, pends_tmp));
                                                }
                                            }
                                            "depend" => {
                                                depend_xml_parser(
                                                    script,
                                                    &attributes,
                                                    &mut depends,
                                                    worker,
                                                )?;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                Token::ElementEnd { end, .. } => {
                                    deps -= 1;
                                    if deps < 0 {
                                        return Err(anyhow!("invalid XML"));
                                    }
                                    if let ElementEnd::Close(prefix, local) = end {
                                        if prefix.as_str() == "" && local.as_str() == "collection" {
                                            break;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        let row: i64 = crate::attr_parse_or_static_string_xml_parser(
                            worker,
                            &attributes,
                            "row",
                        )
                        .parse()
                        .unwrap_or(0);

                        let is_delete = if let Some(v) =
                            attributes.get(&("".to_string(), "delete".to_string()))
                        {
                            v == "1"
                        } else {
                            false
                        };
                        let (collection_id, row) = if row < 0 {
                            (-collection_id, (-row) as u32)
                        } else {
                            (collection_id, row as u32)
                        };
                        if is_delete {
                            updates.push(Record::Delete { collection_id, row });
                        } else {
                            let activity = crate::attr_parse_or_static_xml_parser(
                                worker,
                                &attributes,
                                "activity",
                            );
                            let activity = match &*activity {
                                b"inactive" => Activity::Inactive,
                                b"0" => Activity::Inactive,
                                _ => Activity::Active,
                            };
                            let term_begin = crate::attr_parse_or_static_string_xml_parser(
                                worker,
                                &attributes,
                                "term_begin",
                            );
                            let term_begin = if term_begin != "" {
                                if let Some(t) = chrono::Local
                                    .datetime_from_str(&term_begin, "%Y-%m-%d %H:%M:%S")
                                    .map_or(None, |v| Some(v.timestamp()))
                                {
                                    Term::Overwrite(t as u64)
                                } else {
                                    Term::Default
                                }
                            } else {
                                Term::Default
                            };
                            let term_end = crate::attr_parse_or_static_string_xml_parser(
                                worker,
                                &attributes,
                                "term_end",
                            );
                            let term_end = if term_end != "" {
                                if let Some(t) = chrono::Local
                                    .datetime_from_str(&term_end, "%Y-%m-%d %H:%M:%S")
                                    .map_or(None, |v| Some(v.timestamp()))
                                {
                                    Term::Overwrite(t as u64)
                                } else {
                                    Term::Default
                                }
                            } else {
                                Term::Default
                            };

                            let mut f = Vec::new();
                            for (key, value) in fields {
                                f.push(KeyValue::new(key, value.as_bytes()))
                            }
                            if row == 0 {
                                updates.push(Record::New {
                                    collection_id,
                                    activity,
                                    term_begin,
                                    term_end,
                                    fields: f,
                                    depends: Depends::Overwrite(depends),
                                    pends,
                                });
                            } else {
                                updates.push(Record::Update {
                                    collection_id,
                                    row,
                                    activity,
                                    term_begin,
                                    term_end,
                                    fields: f,
                                    depends: if crate::attr_parse_or_static_string_xml_parser(
                                        worker,
                                        &attributes,
                                        "inherit_depend_if_empty",
                                    ) == "true"
                                        && depends.len() == 0
                                    {
                                        Depends::Default
                                    } else {
                                        Depends::Overwrite(depends)
                                    },
                                    pends,
                                });
                            }
                        }
                    }
                }
            }
            Token::ElementEnd {
                end: ElementEnd::Close(prefix, local),
                ..
            } => {
                if prefix.as_str() == "" && local.as_str() == "update" {
                    break;
                }
            }
            _ => {}
        }
    }
    Ok(updates)
}
