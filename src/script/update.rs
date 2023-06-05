use chrono::TimeZone;
use deno_runtime::deno_core::serde_json;
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
    xml_util, IncludeAdaptor,
};

use super::Script;

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

impl<T: IncludeAdaptor> Script<T> {
    pub fn update(
        &mut self,
        xml: &[u8],
        attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    ) -> Result<()> {
        let inner_xml = self.parse(xml)?;
        let updates = self.make_update_struct(inner_xml.as_slice())?;
        if let Some((ref mut session, _)) = self.sessions.last_mut() {
            let session_rows = self
                .database
                .clone()
                .read()
                .unwrap()
                .update(session, updates)?;
            let commit_rows =
                if crate::attr_parse_or_static(&mut self.worker, &attributes, b"commit") == b"1" {
                    self.database.write().unwrap().commit(session)?
                } else {
                    vec![]
                };
            let src = crate::attr_parse_or_static_string(
                &mut self.worker,
                &attributes,
                b"result_callback",
            );
            if src.len() > 0 {
                if let (Ok(json_commit_rows), Ok(json_session_rows)) = (
                    serde_json::to_string(&commit_rows),
                    serde_json::to_string(&session_rows),
                ) {
                    let _ = self.worker.execute_script(
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

    fn depend(
        &mut self,
        attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
        depends: &mut Vec<(String, CollectionRow)>,
    ) -> Result<(), DependError> {
        let key = crate::attr_parse_or_static_string(&mut self.worker, attributes, b"key");
        let collection =
            crate::attr_parse_or_static_string(&mut self.worker, attributes, b"collection");
        let row = crate::attr_parse_or_static_string(&mut self.worker, attributes, b"row");

        if let (Ok(row), Some(collection_id)) = (
            row.parse::<i64>(),
            self.database
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
                    if let Some(session) = self.sessions.pop() {
                        if let Some(temporary_collection) =
                            session.0.temporary_collection(collection_id)
                        {
                            if let Some(_) = temporary_collection.get(&row) {
                                valid = true;
                            }
                        }
                        self.sessions.push(session);
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
                        let attributes = crate::attr2map(&token_collection.attributes());
                        if let Some((None, Some(collection_name))) =
                            attributes.get(b"name".as_slice())
                        {
                            let collection_id = self
                                .database
                                .clone()
                                .write()
                                .unwrap()
                                .collection_id_or_create(std::str::from_utf8(collection_name)?)
                                .unwrap();

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
                                        let name = token.name();
                                        if let None = name.namespace_prefix() {
                                            match name.local().as_bytes() {
                                                b"field" => {
                                                    let field_name =
                                                        crate::attr_parse_or_static_string(
                                                            &mut self.worker,
                                                            &crate::attr2map(&token.attributes()),
                                                            b"name",
                                                        );
                                                    let (inner_xml, outer_end) =
                                                        xml_util::inner(xml);
                                                    xml = &xml[outer_end..];
                                                    fields.insert(
                                                        field_name,
                                                        std::str::from_utf8(inner_xml)?
                                                            .replace("&gt;", ">")
                                                            .replace("&lt;", "<")
                                                            .replace("&#039;", "'")
                                                            .replace("&quot;", "\"")
                                                            .replace("&amp;", "&"),
                                                    );
                                                }
                                                b"pends" => {
                                                    let (inner_xml, outer_end) =
                                                        xml_util::inner(xml);
                                                    xml = &xml[outer_end..];
                                                    let pends_tmp =
                                                        self.make_update_struct(inner_xml)?;
                                                    if let Some((None, Some(key))) =
                                                        crate::attr2map(&token.attributes())
                                                            .get(&b"key".to_vec())
                                                    {
                                                        pends.push(Pend::new(
                                                            std::str::from_utf8(key)?,
                                                            pends_tmp,
                                                        ));
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    State::ScannedEmptyElementTag(pos) => {
                                        let token_bytes = &xml[..pos];
                                        xml = &xml[pos..];
                                        let token =
                                            token::borrowed::EmptyElementTag::from(token_bytes);
                                        let name = token.name();
                                        if let None = name.namespace_prefix() {
                                            match name.local().as_bytes() {
                                                b"depend" => {
                                                    self.depend(
                                                        &crate::attr2map(&token.attributes()),
                                                        &mut depends,
                                                    )?;
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    State::ScannedEndTag(_) => {
                                        deps -= 1;
                                        if deps < 0 {
                                            return Err(anyhow!("invalid XML"));
                                        }
                                        break;
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
                            let row: i64 = crate::attr_parse_or_static_string(
                                &mut self.worker,
                                &attributes,
                                b"row",
                            )
                            .parse()
                            .unwrap_or(0);

                            let is_delete = if let Some((None, Some(v))) =
                                attributes.get(b"delete".as_slice())
                            {
                                v == b"1"
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
                                let activity = crate::attr_parse_or_static(
                                    &mut self.worker,
                                    &attributes,
                                    b"activity",
                                );
                                let activity = match &*activity {
                                    b"inactive" => Activity::Inactive,
                                    b"0" => Activity::Inactive,
                                    _ => Activity::Active,
                                };
                                let term_begin = crate::attr_parse_or_static_string(
                                    &mut self.worker,
                                    &attributes,
                                    b"term_begin",
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
                                let term_end = crate::attr_parse_or_static_string(
                                    &mut self.worker,
                                    &attributes,
                                    b"term_end",
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
                                        depends: if crate::attr_parse_or_static_string(
                                            &mut self.worker,
                                            &attributes,
                                            b"inherit_depend_if_empty",
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
