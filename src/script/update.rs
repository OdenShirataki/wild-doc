use chrono::TimeZone;
use deno_runtime::{deno_core::error::AnyError, worker::MainWorker};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use semilattice_database::{Activity, Depends, KeyValue, Pend, Record, SessionCollectionRow, Term};
use std::collections::HashMap;

use crate::xml_util;

use super::Script;

pub fn update<T: crate::IncludeAdaptor>(
    script: &mut Script,
    worker: &mut MainWorker,
    reader: &mut Reader<&[u8]>,
    e: &BytesStart,
    include_adaptor: &mut T,
) -> Result<(), AnyError> {
    let with_commit =
        crate::attr_parse_or_static(worker, &xml_util::attr2hash_map(&e), "commit") == b"1";
    let inner_xml = script.parse(worker, reader, b"wd:update", include_adaptor)?;
    let mut inner_reader = Reader::from_str(std::str::from_utf8(&inner_xml).unwrap());
    inner_reader.check_end_names(false);
    let updates = make_update_struct(script, &mut inner_reader, worker);
    if let Some((ref mut session, _)) = script.sessions.last_mut() {
        script
            .database
            .clone()
            .read()
            .unwrap()
            .update(session, updates)?;
        if with_commit {
            script.database.clone().write().unwrap().commit(session)?;
        }
    }
    Ok(())
}

fn depend(
    script: &mut Script,
    e: &BytesStart,
    depends: &mut Vec<(String, SessionCollectionRow)>,
    worker: &mut MainWorker,
) {
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
        depends.push((
            key.to_owned(),
            SessionCollectionRow::new(collection_id, row),
        ));
    }
}
fn make_update_struct(
    script: &mut Script,
    reader: &mut Reader<&[u8]>,
    worker: &mut MainWorker,
) -> Vec<Record> {
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
                            loop {
                                match reader.read_event() {
                                    Ok(Event::Start(ref e)) => {
                                        let name = e.name();
                                        let name_ref = name.as_ref();
                                        if name_ref == b"field" {
                                            if let (Ok(Some(field_name)), Ok(cont)) = (
                                                e.try_get_attribute("name"),
                                                reader.read_text(name),
                                            ) {
                                                if let Ok(field_name) =
                                                    std::str::from_utf8(&field_name.value)
                                                {
                                                    fields.insert(field_name.to_owned(), cont);
                                                }
                                            }
                                        } else if name_ref == b"pends" {
                                            if let Ok(inner_xml) = reader.read_text(name) {
                                                let mut reader_inner = Reader::from_str(&inner_xml);
                                                reader_inner.check_end_names(false);
                                                let pends_tmp = make_update_struct(
                                                    script,
                                                    &mut reader_inner,
                                                    worker,
                                                );
                                                if let Ok(Some(key)) = e.try_get_attribute("key") {
                                                    if let Ok(key) = std::str::from_utf8(&key.value)
                                                    {
                                                        pends.push(Pend::new(key, pends_tmp));
                                                    }
                                                }
                                            }
                                        } else if name_ref == b"depend" {
                                            depend(script, e, &mut depends, worker);
                                        }
                                    }
                                    Ok(Event::Empty(ref e)) => {
                                        if e.name().as_ref() == b"depend" {
                                            depend(script, e, &mut depends, worker);
                                        }
                                    }
                                    Ok(Event::End(ref e)) => {
                                        if e.name().as_ref() == b"collection" {
                                            break;
                                        }
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
                                        Term::Defalut
                                    }
                                } else {
                                    Term::Defalut
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
                                        Term::Defalut
                                    }
                                } else {
                                    Term::Defalut
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
                                        depends: Depends::Overwrite(depends),
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
    updates
}
