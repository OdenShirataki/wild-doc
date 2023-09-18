use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use bson::Bson;

use super::{AttributeMap, Parser, SessionState};

impl Parser {
    #[inline(always)]
    pub(super) fn sessions(&mut self, attributes: &AttributeMap) {
        let mut json = HashMap::new();

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            if let Some(var) = var.as_str() {
                if var != "" {
                    let sessions = self.database.read().unwrap().sessions();
                    json.insert(
                        var.as_bytes().to_vec(),
                        Arc::new(RwLock::new(Bson::Array(
                            sessions
                                .iter()
                                .map(|v| {
                                    let mut doc = bson::Document::new();
                                    doc.insert("name", v.name().clone());
                                    doc.insert(
                                        "access_at",
                                        bson::Timestamp {
                                            time: v.access_at() as u32,
                                            increment: 0,
                                        },
                                    );
                                    doc.insert("expire", v.expire());
                                    Bson::Document(doc)
                                })
                                .collect(),
                        ))),
                    );
                }
            }
        }
        self.state.stack().write().unwrap().push(json);
    }

    #[inline(always)]
    pub(super) fn session(&mut self, attributes: AttributeMap) {
        if let Some(Some(session_name)) = attributes.get(b"name".as_ref()) {
            if let Some(session_name) = session_name.as_str() {
                if session_name != "" {
                    let commit_on_close = attributes
                        .get(b"commit_on_close".as_ref())
                        .and_then(|v| v.as_ref())
                        .map_or(false, |v| v.as_bool().unwrap_or(false));

                    let clear_on_close = attributes
                        .get(b"clear_on_close".as_ref())
                        .and_then(|v| v.as_ref())
                        .map_or(false, |v| v.as_bool().unwrap_or(false));

                    let expire = attributes
                        .get(b"expire".as_ref())
                        .and_then(|v| v.as_ref())
                        .map_or_else(|| "", |v| v.as_str().map_or("", |v| v));
                    let expire = if expire.len() > 0 {
                        expire.parse::<i64>().ok()
                    } else {
                        None
                    };
                    let mut session = self.database.read().unwrap().session(&session_name, expire);
                    if let Some(Some(cursor)) = attributes.get(b"cursor".as_ref()) {
                        if let Some(cursor) = cursor.as_str() {
                            if cursor != "" {
                                if let Ok(cursor) = cursor.parse::<usize>() {
                                    session.set_sequence_cursor(cursor)
                                }
                            }
                        }
                    }
                    if let Some(Some(initialize)) = attributes.get(b"initialize".as_ref()) {
                        if let Some(initialize) = initialize.as_bool() {
                            if initialize {
                                self.database
                                    .clone()
                                    .read()
                                    .unwrap()
                                    .session_restart(&mut session, expire);
                            }
                        }
                    }
                    self.sessions.push(SessionState {
                        session,
                        commit_on_close,
                        clear_on_close,
                    });
                }
            }
        }
    }

    #[inline(always)]
    pub(super) fn session_sequence(&mut self, attributes: AttributeMap) {
        let mut str_max = attributes
            .get(b"max".as_ref())
            .and_then(|v| v.as_ref())
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if str_max == "" {
            str_max = "session_sequence_max";
        }

        let mut str_current = attributes
            .get(b"current".as_ref())
            .and_then(|v| v.as_ref())
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if str_current == "" {
            str_current = "session_sequence_current";
        }

        let mut bson = HashMap::new();
        if let Some(session_state) = self.sessions.last() {
            if let Some(cursor) = session_state.session.sequence_cursor() {
                bson.insert(
                    str_max.as_bytes().to_vec(),
                    Arc::new(RwLock::new(Bson::Int32(cursor.max as i32))),
                );

                bson.insert(
                    str_current.as_bytes().to_vec(),
                    Arc::new(RwLock::new(Bson::Int32(cursor.current as i32))),
                );
            }
        }
        self.state.stack().write().unwrap().push(bson);
    }

    #[inline(always)]
    pub(super) fn session_gc(&mut self, attributes: AttributeMap) {
        self.database.write().unwrap().session_gc(
            attributes
                .get(b"expire".as_ref())
                .and_then(|v| v.as_ref())
                .and_then(|v| v.as_str())
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(60 * 60 * 24),
        );
    }
}
