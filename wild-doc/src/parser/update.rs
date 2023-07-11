use chrono::TimeZone;
use maybe_xml::{
    scanner::{Scanner, State},
    token,
};
use semilattice_database_session::{
    Activity, CollectionRow, Depends, KeyValue, Pend, Record, Term,
};
use std::{collections::HashMap, error, fmt};

use crate::{
    anyhow::{anyhow, Result},
    xml_util,
};

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
        let inner_xml = self.parse(xml)?;
        let updates = self.make_update_struct(inner_xml.as_slice())?;
        if let Some(ref mut session_state) = self.sessions.last_mut() {
            let session_rows = self
                .database
                .clone()
                .read()
                .unwrap()
                .update(&mut session_state.session, updates)?;
            let mut commit_rows = vec![];
            if let Some(Some(commit)) = attributes.get(b"commit".as_ref()) {
                if commit.to_str() == "1" {
                    commit_rows = self
                        .database
                        .write()
                        .unwrap()
                        .commit(&mut session_state.session)?;
                }
            }
            if let Some(Some(name)) = attributes.get(b"rows_set_global".as_ref()) {
                let mut value = serde_json::Map::new();
                value.insert("commit_rows".to_owned(), serde_json::json!(commit_rows));
                value.insert("session_rows".to_owned(), serde_json::json!(session_rows));
                self.register_global(name.to_str().as_ref(), &value.into());
            }
        }
        Ok(())
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
                if row == 0 {
                    return Err(DependError);
                } else {
                    let in_session = row < 0;
                    if in_session {
                        let mut valid = false;
                        if let Some(session_state) = self.sessions.pop() {
                            if let Some(temporary_collection) =
                                session_state.session.temporary_collection(collection_id)
                            {
                                if let Some(_) = temporary_collection.get(&row) {
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
                        key.to_str().into_owned(),
                        if in_session {
                            CollectionRow::new(-collection_id, (-row) as u32)
                        } else {
                            CollectionRow::new(collection_id, row as u32)
                        },
                    ));
                }
                return Ok(());
            }
        }
        Err(DependError)
    }

    fn make_update_struct(&mut self, xml: &[u8]) -> Result<Vec<Record>> {
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
                                .collection_id_or_create(collection_name.to_str().as_ref())?;

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
                                                    fields.insert(
                                                        field_name.to_str().into_owned(),
                                                        std::str::from_utf8(inner_xml)?
                                                            .replace("&gt;", ">")
                                                            .replace("&lt;", "<")
                                                            .replace("&#039;", "'")
                                                            .replace("&quot;", "\"")
                                                            .replace("&amp;", "&"),
                                                    );
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
                                                    pends.push(Pend::new(key.to_str(), pends_tmp));
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
                            let mut row: i64 = 0;
                            if let Some(Some(str_row)) = token_attributes.get(b"row".as_ref()) {
                                if let Ok(parsed) = str_row.to_str().parse::<i64>() {
                                    row = parsed;
                                }
                            }
                            let mut is_delete = false;
                            if let Some(Some(str)) = token_attributes.get(b"delete".as_ref()) {
                                is_delete = str.to_str() == "1";
                            }
                            let (collection_id, row) = if row < 0 {
                                (-collection_id, (-row) as u32)
                            } else {
                                (collection_id, row as u32)
                            };
                            if is_delete {
                                updates.push(Record::Delete { collection_id, row });
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
                                        if let Some(t) = chrono::Local
                                            .datetime_from_str(str.as_ref(), "%Y-%m-%d %H:%M:%S")
                                            .map_or(None, |v| Some(v.timestamp()))
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
                                        if let Some(t) = chrono::Local
                                            .datetime_from_str(str.as_ref(), "%Y-%m-%d %H:%M:%S")
                                            .map_or(None, |v| Some(v.timestamp()))
                                        {
                                            term_end = Term::Overwrite(t as u64)
                                        }
                                    }
                                }
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
                                    let mut inherit_depend_if_empty = false;
                                    if let Some(Some(str)) =
                                        token_attributes.get(b"inherit_depend_if_empty".as_ref())
                                    {
                                        inherit_depend_if_empty = str.to_str() == "true";
                                    }
                                    updates.push(Record::Update {
                                        collection_id,
                                        row,
                                        activity,
                                        term_begin,
                                        term_end,
                                        fields: f,
                                        depends: if inherit_depend_if_empty && depends.len() == 0 {
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
