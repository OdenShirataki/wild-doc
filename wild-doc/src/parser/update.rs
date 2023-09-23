use std::{collections::HashMap, error, fmt};

use anyhow::{anyhow, Result};
use base64::{
    alphabet,
    engine::{self, general_purpose},
    Engine,
};
use chrono::DateTime;
use indexmap::IndexMap;
use maybe_xml::{
    scanner::{Scanner, State},
    token,
};

use semilattice_database_session::{
    Activity, CollectionRow, Depends, KeyValue, Pend, Record, SessionRecord, Term,
};

use wild_doc_script::WildDocValue;

use crate::xml_util;

use super::{AttributeMap, Parser};

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

impl Parser {
    pub fn update(&mut self, xml: &[u8], attributes: &AttributeMap) -> Result<()> {
        if let Ok(inner_xml) = self.parse(xml) {
            let updates = self.make_update_struct(inner_xml.as_slice())?;

            if !self.sessions.last().is_some()
                || attributes
                    .get(b"without_session".as_ref())
                    .and_then(|v| v.as_ref())
                    .and_then(|v| v.as_bool())
                    .cloned()
                    .unwrap_or(false)
            {
                let mut commit_rows = vec![];
                for record in updates {
                    match record {
                        SessionRecord::New {
                            collection_id,
                            record,
                            depends,
                            pends,
                        } => {
                            commit_rows.extend(self.record_new(
                                collection_id,
                                &record,
                                &depends,
                                &pends,
                            ));
                        }
                        SessionRecord::Update {
                            collection_id,
                            row,
                            record,
                            depends,
                            pends,
                        } => {
                            commit_rows.extend(self.record_update(
                                collection_id,
                                row,
                                &record,
                                &depends,
                                &pends,
                            ));
                        }
                        SessionRecord::Delete { collection_id, row } => {
                            if collection_id > 0 {
                                self.database
                                    .write()
                                    .unwrap()
                                    .delete_recursive(&CollectionRow::new(collection_id, row));
                            }
                        }
                    }
                }
                if let Some(Some(name)) = attributes.get(b"rows_set_global".as_ref()) {
                    let mut value = IndexMap::new();
                    value.insert(
                        "commit_rows".to_owned(),
                        WildDocValue::from(
                            commit_rows
                                .iter()
                                .map(|v| {
                                    let mut map = IndexMap::new();
                                    map.insert(
                                        "collection_id".to_owned(),
                                        WildDocValue::from(serde_json::Number::from(
                                            v.collection_id(),
                                        )),
                                    );
                                    map.insert(
                                        "row".to_owned(),
                                        WildDocValue::from(serde_json::Number::from(v.row())),
                                    );
                                    WildDocValue::from(map)
                                })
                                .collect::<Vec<WildDocValue>>(),
                        ),
                    );
                    value.insert("session_rows".to_owned(), WildDocValue::Array(vec![]));
                    self.register_global(name.to_str().as_ref(), &value.into());
                }
            } else {
                if let Some(ref mut session_state) = self.sessions.last_mut() {
                    let session_rows = self
                        .database
                        .clone()
                        .read()
                        .unwrap()
                        .update(&mut session_state.session, updates);
                    let mut commit_rows = vec![];
                    if let Some(Some(commit)) = attributes.get(b"commit".as_ref()) {
                        if commit.as_bool().cloned().unwrap_or(false) {
                            commit_rows = self
                                .database
                                .write()
                                .unwrap()
                                .commit(&mut session_state.session);
                        }
                    }
                    if let Some(Some(name)) = attributes.get(b"rows_set_global".as_ref()) {
                        let mut value = IndexMap::new();
                        value.insert(
                            "commit_rows".to_owned(),
                            WildDocValue::from(
                                commit_rows
                                    .iter()
                                    .map(|v| {
                                        let mut map = IndexMap::new();
                                        map.insert(
                                            "collection_id".to_owned(),
                                            WildDocValue::from(serde_json::Number::from(
                                                v.collection_id(),
                                            )),
                                        );
                                        map.insert(
                                            "row".to_owned(),
                                            WildDocValue::from(serde_json::Number::from(v.row())),
                                        );
                                        WildDocValue::from(map)
                                    })
                                    .collect::<Vec<WildDocValue>>(),
                            ),
                        );
                        value.insert(
                            "session_rows".to_owned(),
                            WildDocValue::from(
                                session_rows
                                    .iter()
                                    .map(|v| {
                                        let mut map = IndexMap::new();
                                        map.insert(
                                            "collection_id".to_owned(),
                                            WildDocValue::from(serde_json::Number::from(
                                                v.collection_id(),
                                            )),
                                        );
                                        map.insert(
                                            "row".to_owned(),
                                            WildDocValue::from(serde_json::Number::from(v.row())),
                                        );
                                        WildDocValue::from(map)
                                    })
                                    .collect::<Vec<WildDocValue>>(),
                            ),
                        );
                        self.register_global(name.to_str().as_ref(), &value.into());
                    }
                }
            }
        }
        Ok(())
    }

    fn update_pends(&mut self, depend: CollectionRow, pends: &Vec<Pend>) -> Vec<CollectionRow> {
        let mut rows = vec![];
        for pend in pends {
            let pend_key = pend.key();
            for record in pend.records() {
                match record {
                    SessionRecord::New {
                        collection_id,
                        record,
                        depends,
                        pends,
                    } => {
                        let mut depends = if let Depends::Overwrite(depends) = depends {
                            depends.clone()
                        } else {
                            Vec::new()
                        };
                        depends.push((pend_key.to_owned(), depend.clone()));

                        rows.extend(self.record_new(
                            *collection_id,
                            record,
                            &Depends::Overwrite(depends),
                            pends,
                        ));
                    }
                    SessionRecord::Update {
                        collection_id,
                        row,
                        record,
                        depends,
                        pends,
                    } => {
                        let mut depends = if let Depends::Overwrite(depends) = depends {
                            depends.clone()
                        } else {
                            Vec::new()
                        };
                        depends.push((pend_key.to_owned(), depend.clone()));

                        rows.extend(self.record_update(
                            *collection_id,
                            *row,
                            record,
                            &Depends::Overwrite(depends),
                            pends,
                        ));
                    }
                    _ => unreachable!(),
                }
            }
        }
        rows
    }

    fn record_new(
        &mut self,
        collection_id: i32,
        record: &Record,
        depends: &Depends,
        pends: &Vec<Pend>,
    ) -> Vec<CollectionRow> {
        let mut rows = vec![];
        if collection_id > 0 {
            let collection_row = self
                .database
                .write()
                .unwrap()
                .collection_mut(collection_id)
                .map(|v| CollectionRow::new(collection_id, v.create_row(record)));
            if let Some(collection_row) = collection_row {
                if let Depends::Overwrite(depends) = depends {
                    for (depend_key, depend_row) in depends {
                        self.database.write().unwrap().register_relation(
                            depend_key,
                            depend_row,
                            collection_row.clone(),
                        );
                    }
                }
                rows.push(collection_row.clone());
                self.update_pends(collection_row, pends);
            }
        }
        rows
    }
    fn record_update(
        &mut self,
        collection_id: i32,
        row: u32,
        record: &Record,
        depends: &Depends,
        pends: &Vec<Pend>,
    ) -> Vec<CollectionRow> {
        let mut rows = vec![];
        if collection_id > 0 {
            let collection_row = self
                .database
                .write()
                .unwrap()
                .collection_mut(collection_id)
                .map(|v| {
                    v.update_row(row, record);
                    CollectionRow::new(collection_id, row)
                });
            if let Some(collection_row) = collection_row {
                if let Depends::Overwrite(depends) = depends {
                    self.database
                        .write()
                        .unwrap()
                        .relation()
                        .write()
                        .unwrap()
                        .delete_pends_by_collection_row(&collection_row);
                    depends.iter().for_each(|d| {
                        self.database.write().unwrap().register_relation(
                            &d.0,
                            &d.1,
                            collection_row.clone(),
                        );
                    });
                }
                rows.push(collection_row.clone());
                self.update_pends(collection_row, &pends);
            }
        }
        rows
    }

    fn depend(
        &mut self,
        attributes: &AttributeMap,
        depends: &mut Vec<(String, CollectionRow)>,
    ) -> Result<(), DependError> {
        if let (Some(Some(key)), Some(Some(collection)), Some(Some(row))) = (
            attributes.get(b"key".as_ref()),
            attributes.get(b"collection".as_ref()),
            attributes.get(b"row".as_ref()),
        ) {
            if let (Ok(row), Some(collection_id)) = (
                row.to_str().parse::<i64>(),
                self.database
                    .clone()
                    .read()
                    .unwrap()
                    .collection_id(&collection.to_str()),
            ) {
                return if row == 0 {
                    Err(DependError)
                } else {
                    let in_session = row < 0;
                    if in_session {
                        let mut valid = false;
                        if let Some(session_state) = self.sessions.pop() {
                            if let Some(temporary_collection) =
                                session_state.session.temporary_collection(collection_id)
                            {
                                if temporary_collection.get(&row).is_some() {
                                    valid = true;
                                }
                            }
                            self.sessions.push(session_state);
                        }
                        if !valid {
                            return Err(DependError);
                        }
                    }
                    depends.push((
                        key.to_string(),
                        if in_session {
                            CollectionRow::new(-collection_id, (-row) as u32)
                        } else {
                            CollectionRow::new(collection_id, row as u32)
                        },
                    ));
                    Ok(())
                };
            }
        }
        Err(DependError)
    }

    fn make_update_struct(&mut self, xml: &[u8]) -> Result<Vec<SessionRecord>> {
        let mut updates = Vec::new();
        let mut xml = xml;
        let mut scanner = Scanner::new();
        while let Some(state) = scanner.scan(&xml) {
            match state {
                State::ScannedStartTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token_collection = token::borrowed::StartTag::from(token_bytes);
                    if token_collection.name().as_bytes() == b"collection" {
                        let token_attributes = self.parse_attibutes(&token_collection.attributes());
                        if let Some(Some(collection_name)) = token_attributes.get(b"name".as_ref())
                        {
                            let collection_id = self
                                .database
                                .clone()
                                .write()
                                .unwrap()
                                .collection_id_or_create(&collection_name.to_str());

                            let mut pends = Vec::new();
                            let mut depends = Vec::new();
                            let mut fields = HashMap::new();
                            let mut deps = 1;
                            while let Some(state) = scanner.scan(&xml) {
                                match state {
                                    State::ScannedStartTag(pos) => {
                                        deps += 1;

                                        let token_bytes = &xml[..pos];
                                        xml = &xml[pos..];
                                        let token = token::borrowed::StartTag::from(token_bytes);
                                        let attributes = self.parse_attibutes(&token.attributes());
                                        let name = token.name();
                                        match name.as_bytes() {
                                            b"field" => {
                                                let (inner_xml, outer_end) = xml_util::inner(xml);
                                                xml = &xml[outer_end..];

                                                if let Some(Some(field_name)) =
                                                    attributes.get(b"name".as_ref())
                                                {
                                                    let mut value = std::str::from_utf8(inner_xml)?
                                                        .replace("&gt;", ">")
                                                        .replace("&lt;", "<")
                                                        .replace("&#039;", "'")
                                                        .replace("&quot;", "\"")
                                                        .replace("&amp;", "&")
                                                        .into_bytes();

                                                    if let Some(Some(base64_decode)) =
                                                        attributes.get(b"base64".as_ref())
                                                    {
                                                        if base64_decode
                                                            .as_bool()
                                                            .cloned()
                                                            .unwrap_or(false)
                                                        {
                                                            value = engine::GeneralPurpose::new(
                                                                &alphabet::STANDARD,
                                                                general_purpose::NO_PAD,
                                                            )
                                                            .decode(value)
                                                            .unwrap();
                                                        }
                                                    }
                                                    fields.insert(field_name.to_string(), value);
                                                }
                                            }
                                            b"pends" => {
                                                let (inner_xml, outer_end) = xml_util::inner(xml);
                                                xml = &xml[outer_end..];
                                                let pends_tmp =
                                                    self.make_update_struct(inner_xml)?;

                                                if let Some(Some(key)) =
                                                    attributes.get(b"key".as_ref())
                                                {
                                                    pends.push(Pend::new(
                                                        key.to_string(),
                                                        pends_tmp,
                                                    ));
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    State::ScannedEmptyElementTag(pos) => {
                                        let token_bytes = &xml[..pos];
                                        xml = &xml[pos..];
                                        let token =
                                            token::borrowed::EmptyElementTag::from(token_bytes);
                                        let name = token.name();
                                        match name.as_bytes() {
                                            b"depend" => {
                                                let attributes =
                                                    self.parse_attibutes(&token.attributes());
                                                self.depend(&attributes, &mut depends)?;
                                            }
                                            _ => {}
                                        }
                                    }
                                    State::ScannedEndTag(pos) => {
                                        let token_bytes = &xml[..pos];
                                        xml = &xml[pos..];
                                        if token::borrowed::EndTag::from(token_bytes)
                                            .name()
                                            .as_bytes()
                                            == b"collection"
                                        {
                                            break;
                                        }
                                        deps -= 1;
                                        if deps < 0 {
                                            return Err(anyhow!("invalid XML"));
                                        }
                                    }
                                    State::ScannedCharacters(pos)
                                    | State::ScannedCdata(pos)
                                    | State::ScannedComment(pos)
                                    | State::ScannedDeclaration(pos)
                                    | State::ScannedProcessingInstruction(pos) => {
                                        xml = &xml[pos..];
                                    }
                                    _ => {
                                        return Err(anyhow!("invalid XML"));
                                    }
                                }
                            }

                            let row: i64 = token_attributes
                                .get(b"row".as_ref())
                                .and_then(|v| v.as_ref())
                                .and_then(|v| v.to_str().parse::<i64>().ok())
                                .unwrap_or(0);

                            let is_delete = token_attributes
                                .get(b"delete".as_ref())
                                .and_then(|v| v.as_ref())
                                .and_then(|v| v.as_bool())
                                .cloned()
                                .unwrap_or(false);
                            let (collection_id, row) = if row < 0 {
                                (-collection_id, (-row) as u32)
                            } else {
                                (collection_id, row as u32)
                            };
                            if is_delete {
                                updates.push(SessionRecord::Delete { collection_id, row });
                            } else {
                                let mut activity = Activity::Active;
                                if let Some(Some(str)) = token_attributes.get(b"activity".as_ref())
                                {
                                    let str = str.to_str();
                                    if str == "inactive" || str == "0" {
                                        activity = Activity::Inactive;
                                    }
                                }
                                let mut term_begin = Term::Default;
                                if let Some(Some(str)) =
                                    token_attributes.get(b"term_begin".as_ref())
                                {
                                    let str = str.to_str();
                                    if str != "" {
                                        if let Ok(t) =
                                            DateTime::parse_from_str(&str, "%Y-%m-%d %H:%M:%S")
                                                .map(|v| v.timestamp())
                                        {
                                            term_begin = Term::Overwrite(t as u64)
                                        }
                                    }
                                }
                                let mut term_end = Term::Default;
                                if let Some(Some(str)) = token_attributes.get(b"term_end".as_ref())
                                {
                                    let str = str.to_str();
                                    if str != "" {
                                        if let Ok(t) =
                                            DateTime::parse_from_str(&str, "%Y-%m-%d %H:%M:%S")
                                                .map(|v| v.timestamp())
                                        {
                                            term_end = Term::Overwrite(t as u64)
                                        }
                                    }
                                }
                                let record = Record {
                                    activity,
                                    term_begin,
                                    term_end,
                                    fields: fields
                                        .iter()
                                        .map(|(key, value)| KeyValue::new(key, value.clone()))
                                        .collect(),
                                };
                                updates.push(if row == 0 {
                                    SessionRecord::New {
                                        collection_id,
                                        record,
                                        depends: Depends::Overwrite(depends),
                                        pends,
                                    }
                                } else {
                                    let inherit_depend_if_empty = if let Some(Some(str)) =
                                        token_attributes.get(b"inherit_depend_if_empty".as_ref())
                                    {
                                        str.as_bool().cloned().unwrap_or(false)
                                    } else {
                                        false
                                    };
                                    SessionRecord::Update {
                                        collection_id,
                                        row,
                                        record,
                                        depends: if inherit_depend_if_empty && depends.len() == 0 {
                                            Depends::Default
                                        } else {
                                            Depends::Overwrite(depends)
                                        },
                                        pends,
                                    }
                                });
                            }
                        }
                    }
                }
                State::ScannedEndTag(_) => {
                    break;
                }
                State::ScannedCharacters(pos)
                | State::ScannedCdata(pos)
                | State::ScannedComment(pos)
                | State::ScannedDeclaration(pos)
                | State::ScannedProcessingInstruction(pos) => {
                    xml = &xml[pos..];
                }
                _ => {
                    break;
                }
            }
        }
        Ok(updates)
    }
}
