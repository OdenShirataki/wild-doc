use chrono::TimeZone;
use deno_runtime::{
    deno_core::{anyhow::anyhow, error::AnyError, serde_json},
    worker::MainWorker,
};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use semilattice_database::{Activity, Depends, KeyValue, Pend, Record, SessionCollectionRow, Term};
use std::{collections::HashMap, error, fmt};

use crate::xml_util;

use super::Script;

pub fn update<T: crate::IncludeAdaptor>(
    script: &mut Script,
    worker: &mut MainWorker,
    reader: &mut Reader<&[u8]>,
    e: &BytesStart,
    include_adaptor: &mut T,
) -> Result<(), AnyError> {
    //TODO: Will the session data be corrupted if there is an update that makes depend empty from the state where depend exists?
    //TODO: break relations after commit?
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
                let code = "{const update_result={commit_rows:".to_owned()
                    + json_commit_rows.as_str()
                    + ",session_rows:"
                    + json_session_rows.as_str()
                    + "};"
                    + src.as_str()
                    + "}";
                let _ = worker.execute_script("commit", code);
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
    depends: &mut Vec<(String, SessionCollectionRow)>,
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
            if row < 0 {
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
                SessionCollectionRow::new(collection_id, row),
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
) -> Result<Vec<Record>, AnyError> {
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
                            let row = crate::attr_parse_or_static_string(worker, &attr, "row")
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
