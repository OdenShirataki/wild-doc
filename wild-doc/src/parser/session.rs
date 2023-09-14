use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, RwLock},
};

use serde_json::json;

use wild_doc_script::WildDocValue;

use super::{AttributeMap, Parser, SessionState};

impl Parser {
    pub(super) fn sessions(&mut self, attributes: &AttributeMap) {
        let mut json = HashMap::new();

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            let var = var.as_bytes();
            if var.as_ref() != b"" {
                let sessions = self.database.read().unwrap().sessions();
                json.insert(
                    var.to_vec(),
                    Arc::new(RwLock::new(WildDocValue::from(json!(sessions)))),
                );
            }
        }
        self.state.stack().write().unwrap().push(json);
    }

    pub(super) fn session(&mut self, attributes: AttributeMap) {
        if let Some(Some(session_name)) = attributes.get(b"name".as_ref()) {
            let session_name = session_name.to_str();
            if session_name != "" {
                let commit_on_close = attributes
                    .get(b"commit_on_close".as_ref())
                    .and_then(|v| v.as_ref())
                    .map_or(false, |v| v.to_str() == "true");

                let clear_on_close = attributes
                    .get(b"clear_on_close".as_ref())
                    .and_then(|v| v.as_ref())
                    .map_or(false, |v| v.to_str() == "true");

                let expire = attributes
                    .get(b"expire".as_ref())
                    .and_then(|v| v.as_ref())
                    .map_or_else(|| "".into(), |v| v.to_str());
                let expire = if expire.len() > 0 {
                    expire.parse::<i64>().ok()
                } else {
                    None
                };
                let mut session = self.database.read().unwrap().session(&session_name, expire);
                if let Some(Some(cursor)) = attributes.get(b"cursor".as_ref()) {
                    let cursor = cursor.to_str();
                    if cursor != "" {
                        if let Ok(cursor) = cursor.parse::<usize>() {
                            session.set_sequence_cursor(cursor)
                        }
                    }
                }
                if let Some(Some(initialize)) = attributes.get(b"initialize".as_ref()) {
                    if initialize.to_str() == "true" {
                        self.database
                            .clone()
                            .read()
                            .unwrap()
                            .session_restart(&mut session, expire);
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

    pub(super) fn session_sequence(&mut self, attributes: AttributeMap) {
        let mut str_max = attributes
            .get(b"max".as_ref())
            .and_then(|v| v.as_ref())
            .map_or(Cow::Borrowed(b"".as_ref()), |v| v.as_bytes());
        if str_max.as_ref() == b"" {
            str_max = Cow::Borrowed(b"session_sequence_max".as_ref());
        }

        let mut str_current = attributes
            .get(b"current".as_ref())
            .and_then(|v| v.as_ref())
            .map_or(Cow::Borrowed(b"".as_ref()), |v| v.as_bytes());
        if str_current.as_ref() == b"" {
            str_current = Cow::Borrowed(b"session_sequence_current");
        }

        let mut json = HashMap::new();
        if let Some(session_state) = self.sessions.last() {
            if let Some(cursor) = session_state.session.sequence_cursor() {
                json.insert(
                    str_max.to_vec(),
                    Arc::new(RwLock::new(WildDocValue::from(json!(cursor.max)))),
                );

                json.insert(
                    str_current.to_vec(),
                    Arc::new(RwLock::new(WildDocValue::from(json!(cursor.current)))),
                );
            }
        }
        self.state.stack().write().unwrap().push(json);
    }
    pub(super) fn session_gc(&mut self, attributes: AttributeMap) {
        self.database.write().unwrap().session_gc(
            attributes
                .get(b"expire".as_ref())
                .and_then(|v| v.as_ref())
                .and_then(|v| v.to_str().parse::<i64>().ok())
                .unwrap_or(60 * 60 * 24),
        );
    }
}
