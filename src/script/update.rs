use std::collections::HashMap;

use chrono::TimeZone;
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use semilattice_database::{Activity, Depends, KeyValue, Pend, Record, SessionCollectionRow, Term};

use deno_runtime::worker::MainWorker;

use crate::xml_util;

use super::Script;

fn depend(script: &mut Script, e: &BytesStart, depends: &mut Vec<(String, SessionCollectionRow)>) {
    if let (Ok(Some(key)), Ok(Some(collection_name)), Ok(Some(row))) = (
        e.try_get_attribute("key"),
        e.try_get_attribute("collection"),
        e.try_get_attribute("row"),
    ) {
        if let (Ok(key), Ok(row), Ok(collection_name)) = (
            std::str::from_utf8(&key.value),
            std::str::from_utf8(&row.value),
            std::str::from_utf8(&collection_name.value),
        ) {
            if let (Ok(row), Some(collection_id)) = (
                row.parse::<i64>(),
                script
                    .database
                    .clone()
                    .read()
                    .unwrap()
                    .collection_id(collection_name),
            ) {
                depends.push((
                    key.to_owned(),
                    SessionCollectionRow::new(collection_id, row),
                ));
            }
        }
    }
}
pub fn make_update_struct(
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
                            let mut pends = Vec::new();
                            let mut depends = Vec::new();
                            let mut fields = HashMap::new();
                            loop {
                                match reader.read_event() {
                                    Ok(Event::Start(ref e)) => {
                                        let name = e.name();
                                        let name_ref = name.as_ref();
                                        if name_ref == b"field" {
                                            if let Ok(Some(field_name)) =
                                                e.try_get_attribute("name")
                                            {
                                                if let Ok(field_name) =
                                                    std::str::from_utf8(&field_name.value)
                                                {
                                                    let cont =
                                                        xml_util::text_content(reader, e.name());
                                                    fields.insert(field_name.to_owned(), cont);
                                                }
                                            }
                                        } else if name_ref == b"pends" {
                                            let inner_xml = xml_util::inner(reader);
                                            let mut reader_inner = Reader::from_str(&inner_xml);
                                            reader_inner.check_end_names(false);
                                            let pends_tmp = make_update_struct(
                                                script,
                                                &mut reader_inner,
                                                worker,
                                            );
                                            if let Ok(Some(key)) = e.try_get_attribute("key") {
                                                if let Ok(key) = std::str::from_utf8(&key.value) {
                                                    pends.push(Pend::new(key, pends_tmp));
                                                }
                                            }
                                        } else if name_ref == b"depend" {
                                            depend(script, e, &mut depends);
                                        }
                                    }
                                    Ok(Event::Empty(ref e)) => {
                                        if e.name().as_ref() == b"depend" {
                                            depend(script, e, &mut depends);
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
                            let row = crate::attr_parse_or_static(worker, &attr, "row")
                                .parse()
                                .unwrap_or(0);

                            let activity = crate::attr_parse_or_static(worker, &attr, "activity");
                            let activity = match &*activity {
                                "inactive" => Activity::Inactive,
                                "0" => Activity::Inactive,
                                _ => Activity::Active,
                            };
                            let term_begin =
                                crate::attr_parse_or_static(worker, &attr, "term_begin");
                            let term_begin = if term_begin != "" {
                                if let Some(t) = chrono::Local
                                    .datetime_from_str(&term_begin, "%Y-%m-%d %H:%M:%S")
                                    .map_or(None, |v| Some(v.timestamp()))
                                {
                                    Term::Overwrite(t)
                                } else {
                                    Term::Defalut
                                }
                            } else {
                                Term::Defalut
                            };
                            let term_end = crate::attr_parse_or_static(worker, &attr, "term_end");
                            let term_end = if term_end != "" {
                                if let Some(t) = chrono::Local
                                    .datetime_from_str(&term_end, "%Y-%m-%d %H:%M:%S")
                                    .map_or(None, |v| Some(v.timestamp()))
                                {
                                    Term::Overwrite(t)
                                } else {
                                    Term::Defalut
                                }
                            } else {
                                Term::Defalut
                            };
                            /*
                            let is_delete=if let Some(v)=attr.get("delete"){
                                if let Ok(v)=std::str::from_utf8(v){
                                    v=="1"
                                }else{
                                    false
                                }
                            }else{
                                false
                            }; */
                            let collection_id = script
                                .database
                                .clone()
                                .write()
                                .unwrap()
                                .collection_id_or_create(collection_name)
                                .unwrap();
                            let mut f = Vec::new();
                            for (key, value) in fields {
                                f.push(KeyValue::new(key, value))
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
