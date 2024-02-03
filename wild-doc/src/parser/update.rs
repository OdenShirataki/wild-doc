use std::{
    error, fmt,
    num::{NonZeroI32, NonZeroI64, NonZeroU32},
    sync::Arc,
};

use anyhow::{anyhow, Result};
use async_recursion::async_recursion;
use base64::{engine::general_purpose, Engine};
use chrono::DateTime;
use hashbrown::HashMap;
use maybe_xml::{token::Ty, Reader};

use semilattice_database_session::{
    Activity, CollectionRow, Depends, FieldName, Pend, SessionRecord, Term,
};

use wild_doc_script::{Vars, WildDocValue};

use crate::{r#const::*,xml_util};

use super::Parser;

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
    fn rows2val(&self, commit_rows: Vec<CollectionRow>) -> WildDocValue {
        WildDocValue::Array(
            commit_rows
                .into_iter()
                .map(|v| {
                    WildDocValue::Object(
                        [
                            (
                                Arc::clone(&*COLLECTION_ID),
                                serde_json::Number::from(v.collection_id().get()).into(),
                            ),
                            (
                                Arc::clone(&*ROW),
                                serde_json::Number::from(v.row().get()).into(),
                            ),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )
    }

    pub async fn update(&mut self, xml: &[u8], pos: &mut usize, attr: Vars) -> Result<Vec<u8>> {
        let mut r = vec![];
        if let Ok(inner_xml) = self.parse(xml, pos).await {
            let mut pos = 0;
            let (updates, on) = self
                .make_update_struct(inner_xml.as_slice(), &mut pos)
                .await?;

            let mut commit_rows = vec![];
            let mut session_rows = vec![];

            if !self.sessions.last().is_some()
                || attr
                    .get(&*WITHOUT_SESSION)
                    .and_then(|v| v.as_bool())
                    .map_or(false, |v| *v)
            {
                for record in updates.into_iter() {
                    match record {
                        SessionRecord::Update {
                            collection_id,
                            row,
                            activity,
                            term_begin,
                            term_end,
                            fields,
                            depends,
                            pends,
                        } => {
                            commit_rows.extend(
                                self.record_update(
                                    collection_id,
                                    row,
                                    activity,
                                    term_begin,
                                    term_end,
                                    fields,
                                    &depends,
                                    pends,
                                )
                                .await,
                            );
                        }
                        SessionRecord::Delete { collection_id, row } => {
                            if collection_id.get() > 0 {
                                self.database
                                    .write()
                                    .delete(&CollectionRow::new(collection_id, row))
                                    .await;
                            }
                        }
                    }
                }
            } else {
                if let Some(session_state) = self.sessions.last_mut() {
                    session_rows = self
                        .database
                        .write()
                        .update(&mut session_state.session, updates)
                        .await;
                    if let Some(commit) = attr.get(&*COMMIT) {
                        if commit.as_bool().map_or(false, |v| *v) {
                            commit_rows = self
                                .database
                                .write()
                                .commit(&mut session_state.session)
                                .await;
                        }
                    }
                }
            }
            if let Some((on_xml, on_vars)) = on {
                let mut new_vars = Vars::new();
                new_vars.insert(
                    if let Some(var) = on_vars.get(&*VAR) {
                        var.as_string()
                    } else {
                        Arc::clone(&*UPDATE)
                    },
                    WildDocValue::Object(
                        [
                            (Arc::clone(&*COMMIT_ROWS), self.rows2val(commit_rows)),
                            (Arc::clone(&*SESSION_ROWS), self.rows2val(session_rows)),
                        ]
                        .into(),
                    ),
                );
                let mut pos = 0;
                self.stack.push(new_vars);
                r = self.parse(on_xml, &mut pos).await?;
                self.stack.pop();
            }
        }
        Ok(r)
    }

    #[async_recursion(?Send)]
    async fn update_pends(&self, depend: &CollectionRow, pends: Vec<Pend>) -> Vec<CollectionRow> {
        let mut rows = vec![];
        for pend in pends.into_iter() {
            let pend_key = pend.key;
            for record in pend.records.into_iter() {
                match record {
                    SessionRecord::Update {
                        collection_id,
                        row,
                        activity,
                        term_begin,
                        term_end,
                        fields,
                        depends,
                        pends,
                    } => {
                        rows.extend(
                            self.record_update(
                                collection_id,
                                row,
                                activity,
                                term_begin,
                                term_end,
                                fields,
                                &Depends::Overwrite(
                                    if let Depends::Overwrite(mut depends) = depends {
                                        depends.push((pend_key.to_owned(), depend.clone()));
                                        depends
                                    } else {
                                        vec![(pend_key.to_owned(), depend.clone())]
                                    },
                                ),
                                pends,
                            )
                            .await,
                        );
                    }
                    _ => unreachable!(),
                }
            }
        }
        rows
    }

    async fn record_update(
        &self,
        collection_id: NonZeroI32,
        row: Option<NonZeroU32>,
        activity: Activity,
        term_begin: Term,
        term_end: Term,
        fields: HashMap<FieldName, Vec<u8>>,
        depends: &Depends,
        pends: Vec<Pend>,
    ) -> Vec<CollectionRow> {
        let mut rows = vec![];
        if let Some(row) = row {
            if collection_id.get() > 0 {
                let collection_row =
                    if let Some(collection) = self.database.write().collection_mut(collection_id) {
                        Arc::new(
                            collection
                                .update(row, activity, term_begin, term_end, fields)
                                .await,
                        );
                        Some(CollectionRow::new(collection_id, row))
                    } else {
                        None
                    };
                if let Some(ref collection_row) = collection_row {
                    if let Depends::Overwrite(depends) = depends {
                        self.database
                            .write()
                            .relation_mut()
                            .delete_pends_by_collection_row(collection_row)
                            .await;
                        for d in depends.into_iter() {
                            self.database
                                .write()
                                .register_relation(&d.0, &d.1, collection_row.clone())
                                .await;
                        }
                    }
                    rows.push(collection_row.clone());
                    self.update_pends(collection_row, pends).await;
                }
            }
        } else {
            if collection_id.get() > 0 {
                let collection_row =
                    if let Some(collection) = self.database.write().collection_mut(collection_id) {
                        Some(CollectionRow::new(
                            collection_id,
                            collection
                                .insert(activity, term_begin, term_end, fields)
                                .await,
                        ))
                    } else {
                        None
                    };
                if let Some(collection_row) = collection_row {
                    if let Depends::Overwrite(depends) = depends {
                        for (depend_key, depend_row) in depends.into_iter() {
                            self.database
                                .write()
                                .register_relation(&depend_key, depend_row, collection_row.clone())
                                .await;
                        }
                    }
                    rows.push(collection_row.clone());
                    self.update_pends(&collection_row, pends).await;
                }
            }
        }
        rows
    }

    fn depend(
        &mut self,
        vars: &Vars,
        depends: &mut Vec<(Arc<String>, CollectionRow)>,
    ) -> Result<(), DependError> {
        if let (Some(key), Some(collection), Some(row)) =
            (vars.get(&*KEY), vars.get(&*COLLECTION), vars.get(&*ROW))
        {
            if let (Ok(row), Some(collection_id)) = (
                row.as_string().parse::<NonZeroI64>(),
                self.database.read().collection_id(&collection.as_string()),
            ) {
                let in_session = row.get() < 0;
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
                    key.as_string(),
                    if in_session {
                        CollectionRow::new(-collection_id, (-row).try_into().unwrap())
                    } else {
                        CollectionRow::new(collection_id, row.try_into().unwrap())
                    },
                ));
                return Ok(());
            }
        }
        Err(DependError)
    }

    #[async_recursion(?Send)]
    async fn make_update_struct<'a, 'b>(
        &mut self,
        xml: &'a [u8],
        pos: &mut usize,
    ) -> Result<(Vec<SessionRecord>, Option<(&'b [u8], Vars)>)>
    where
        'a: 'b,
    {
        let mut updates = Vec::new();
        let mut on = None;

        let reader = Reader::from_str(unsafe { std::str::from_utf8_unchecked(xml) });
        while let Some(token) = reader.tokenize(pos) {
            match token.ty() {
                Ty::StartTag(st) => {
                    match st.name().as_bytes() {
                        b"wd:on" => {
                            let begin = *pos;
                            let (inner, _) = xml_util::to_end(xml, pos);
                            on = Some((
                                &xml[begin..inner],
                                self.vars_from_attibutes(st.attributes()).await,
                            ));
                        }
                        b"collection" => {
                            let attr = self.vars_from_attibutes(st.attributes()).await;
                            if let Some(collection_name) = attr.get(&*NAME) {
                                let collection_id = self
                                    .database
                                    .write()
                                    .collection_id_or_create(&collection_name.as_string());

                                let mut pends = Vec::new();
                                let mut depends = Vec::new();
                                let mut fields = HashMap::new();
                                let mut deps = 1;
                                while let Some(token) = reader.tokenize(pos) {
                                    match token.ty() {
                                        Ty::StartTag(st) => {
                                            deps += 1;

                                            let attr =
                                                self.vars_from_attibutes(st.attributes()).await;
                                            match st.name().as_bytes() {
                                                b"field" => {
                                                    let begin = *pos;
                                                    let (inner, _) = xml_util::to_end(xml, pos);

                                                    if let Some(field_name) = attr.get(&*NAME) {
                                                        let mut value = std::str::from_utf8(
                                                            &xml[begin..inner],
                                                        )?
                                                        .replace("&gt;", ">")
                                                        .replace("&lt;", "<")
                                                        .replace("&#039;", "'")
                                                        .replace("&quot;", "\"")
                                                        .replace("&amp;", "&")
                                                        .into_bytes();

                                                        if let Some(base64_decode) =
                                                            attr.get(&*BASE64)
                                                        {
                                                            if base64_decode
                                                                .as_bool()
                                                                .map_or(false, |v| *v)
                                                            {
                                                                value =
                                                                general_purpose::STANDARD_NO_PAD
                                                                    .decode(value)
                                                                    .unwrap();
                                                            }
                                                        }
                                                        fields.insert(
                                                            FieldName::new(field_name.to_string()),
                                                            value,
                                                        );
                                                    }
                                                }
                                                b"pends" => {
                                                    //TODO: proc for _on_xml?
                                                    let (pends_tmp, _on_xml) =
                                                        self.make_update_struct(xml, pos).await?;

                                                    if let Some(key) = attr.get(&*KEY) {
                                                        pends.push(Pend {
                                                            key: key.as_string(),
                                                            records: pends_tmp,
                                                        });
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                        Ty::EmptyElementTag(eet) => {
                                            let name = eet.name();
                                            match name.as_bytes() {
                                                b"depend" => {
                                                    let attr = self
                                                        .vars_from_attibutes(eet.attributes())
                                                        .await;
                                                    self.depend(&attr, &mut depends)?;
                                                }
                                                _ => {}
                                            }
                                        }
                                        Ty::EndTag(et) => {
                                            if et.name().as_bytes() == b"collection" {
                                                break;
                                            }
                                            deps -= 1;
                                            if deps < 0 {
                                                return Err(anyhow!("invalid XML"));
                                            }
                                        }
                                        Ty::Characters(_)
                                        | Ty::Cdata(_)
                                        | Ty::Comment(_)
                                        | Ty::Declaration(_)
                                        | Ty::ProcessingInstruction(_) => {}
                                    }
                                }

                                let row: i64 = attr
                                    .get(&*ROW)
                                    .and_then(|v| v.as_string().parse::<i64>().ok())
                                    .unwrap_or(0);

                                let (collection_id, row) = if row < 0 {
                                    (-collection_id, (-row) as u32)
                                } else {
                                    (collection_id, row as u32)
                                };
                                if attr
                                    .get(&*DELETE)
                                    .and_then(|v| v.as_bool())
                                    .map_or(false, |v| *v)
                                {
                                    if row != 0 {
                                        updates.push(SessionRecord::Delete {
                                            collection_id,
                                            row: unsafe { NonZeroU32::new_unchecked(row) },
                                        });
                                    }
                                } else {
                                    let mut activity = Activity::Active;
                                    if let Some(str) = attr.get(&*ACTIVITY) {
                                        let str = str.as_string();
                                        let str = str.as_str();
                                        if str == "inactive" || str == "0" {
                                            activity = Activity::Inactive;
                                        }
                                    }
                                    let mut term_begin = Term::Default;
                                    if let Some(str) = attr.get(&*TERM_BEGIN) {
                                        let str = str.as_string();
                                        let str = str.as_str();
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
                                    if let Some(str) = attr.get(&*TERM_END) {
                                        let str = str.as_string();
                                        let str = str.as_str();
                                        if str != "" {
                                            if let Ok(t) =
                                                DateTime::parse_from_str(&str, "%Y-%m-%d %H:%M:%S")
                                                    .map(|v| v.timestamp())
                                            {
                                                term_end = Term::Overwrite(t as u64)
                                            }
                                        }
                                    }
                                    updates.push(if row == 0 {
                                        SessionRecord::Update {
                                            collection_id,
                                            row: None,
                                            activity,
                                            term_begin,
                                            term_end,
                                            fields,
                                            depends: Depends::Overwrite(depends),
                                            pends,
                                        }
                                    } else {
                                        let inherit_depend_if_empty = if let Some(str) =
                                            attr.get(&*INHERIT_DEPEND_IF_EMPTY)
                                        {
                                            str.as_bool().map_or(false, |v| *v)
                                        } else {
                                            false
                                        };
                                        SessionRecord::Update {
                                            collection_id,
                                            row: NonZeroU32::new(row),
                                            activity,
                                            term_begin,
                                            term_end,
                                            fields,
                                            depends: if inherit_depend_if_empty
                                                && depends.len() == 0
                                            {
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
                        _ => {}
                    }
                }
                Ty::EndTag(_) => {
                    break;
                }
                Ty::Characters(_)
                | Ty::Cdata(_)
                | Ty::Comment(_)
                | Ty::Declaration(_)
                | Ty::ProcessingInstruction(_) => {}
                _ => {
                    break;
                }
            }
        }
        Ok((updates, on))
    }
}
