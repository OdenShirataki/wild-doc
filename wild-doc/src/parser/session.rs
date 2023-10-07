use std::{
    borrow::Cow,
    sync::{Arc, RwLock},
};

use hashbrown::HashMap;
use serde_json::json;

use wild_doc_script::WildDocValue;

use super::{AttributeMap, Parser, SessionState};

impl Parser {
    #[inline(always)]
    pub(super) fn sessions(&self, attributes: &AttributeMap) {
        let mut json = HashMap::new();

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            let var = var.to_str();
            if var != "" {
                let sessions = self.database.read().unwrap().sessions();
                json.insert(
                    var.to_string().into_bytes(),
                    Arc::new(RwLock::new(WildDocValue::from(json!(sessions)))),
                );
            }
        }
        self.state.stack().write().unwrap().push(json);
    }

    #[inline(always)]
    pub(super) fn session(&mut self, attributes: AttributeMap) {
        if let Some(Some(session_name)) = attributes.get(b"name".as_ref()) {
            let session_name = session_name.to_str();
            if session_name != "" {
                let commit_on_close = attributes
                    .get(b"commit_on_close".as_ref())
                    .and_then(|v| v.as_ref())
                    .and_then(|v| v.as_bool())
                    .map_or(false, |v| *v);

                let clear_on_close = attributes
                    .get(b"clear_on_close".as_ref())
                    .and_then(|v| v.as_ref())
                    .and_then(|v| v.as_bool())
                    .map_or(false, |v| *v);

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
                    if initialize.as_bool().map_or(false, |v| *v) {
                        self.database
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

    #[inline(always)]
    pub(super) fn session_sequence(&self, attributes: AttributeMap) {
        let mut str_max = attributes
            .get(b"max".as_ref())
            .and_then(|v| v.as_ref())
            .map_or(Cow::Borrowed(""), |v| v.to_str());
        if str_max == "" {
            str_max = Cow::Borrowed("session_sequence_max");
        }

        let mut str_current = attributes
            .get(b"current".as_ref())
            .and_then(|v| v.as_ref())
            .map_or(Cow::Borrowed(""), |v| v.to_str());
        if str_current == "" {
            str_current = Cow::Borrowed("session_sequence_current");
        }

        let mut json = HashMap::new();
        if let Some(session_state) = self.sessions.last() {
            if let Some(cursor) = session_state.session.sequence_cursor() {
                json.insert(
                    str_max.to_string().into_bytes(),
                    Arc::new(RwLock::new(WildDocValue::Number(serde_json::Number::from(
                        cursor.max,
                    )))),
                );
                json.insert(
                    str_current.to_string().into_bytes(),
                    Arc::new(RwLock::new(WildDocValue::Number(serde_json::Number::from(
                        cursor.current,
                    )))),
                );
            }
        }
        self.state.stack().write().unwrap().push(json);
    }

    #[inline(always)]
    pub(super) fn session_gc(&self, attributes: AttributeMap) {
        self.database.write().unwrap().session_gc(
            attributes
                .get(b"expire".as_ref())
                .and_then(|v| v.as_ref())
                .and_then(|v| v.to_str().parse::<i64>().ok())
                .unwrap_or(60 * 60 * 24),
        );
    }
}
