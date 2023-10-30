use std::{borrow::Cow, sync::Arc};

use serde_json::json;

use wild_doc_script::{Vars, WildDocValue};

use super::{Parser, SessionState};

impl Parser {
    #[inline(always)]
    #[must_use]
    pub(super) fn sessions(&self, vars: Vars) -> Vars {
        let mut r = Vars::new();

        if let Some(var) = vars.get("var") {
            let var = var.to_str();
            if var != "" {
                let sessions = self.database.read().sessions();
                r.insert(var.into(), Arc::new(json!(sessions).into()));
            }
        }
        r
    }

    #[inline(always)]
    pub(super) fn session(&mut self, vars: Vars) {
        if let Some(session_name) = vars.get("name") {
            let session_name = session_name.to_str();
            if session_name != "" {
                let commit_on_close = vars
                    .get("commit_on_close")
                    .and_then(|v| v.as_bool())
                    .map_or(false, |v| *v);

                let clear_on_close = vars
                    .get("clear_on_close")
                    .and_then(|v| v.as_bool())
                    .map_or(false, |v| *v);

                let expire = vars.get("expire").map_or_else(|| "".into(), |v| v.to_str());
                let expire = if expire.len() > 0 {
                    expire.parse::<i64>().ok()
                } else {
                    None
                };
                let mut session = self.database.read().session(&session_name, expire);
                if let Some(cursor) = vars.get("cursor") {
                    let cursor = cursor.to_str();
                    if cursor != "" {
                        if let Ok(cursor) = cursor.parse::<usize>() {
                            session.set_sequence_cursor(cursor)
                        }
                    }
                }
                if let Some(initialize) = vars.get("initialize") {
                    if initialize.as_bool().map_or(false, |v| *v) {
                        self.database.read().session_restart(&mut session, expire);
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
    #[must_use]
    pub(super) fn session_sequence(&self, vars: Vars) -> Vars {
        let mut str_max = vars.get("max").map_or(Cow::Borrowed(""), |v| v.to_str());
        if str_max == "" {
            str_max = Cow::Borrowed("session_sequence_max");
        }

        let mut str_current = vars
            .get("current")
            .map_or(Cow::Borrowed(""), |v| v.to_str());
        if str_current == "" {
            str_current = Cow::Borrowed("session_sequence_current");
        }

        let mut r = Vars::new();
        if let Some(session_state) = self.sessions.last() {
            if let Some(cursor) = session_state.session.sequence_cursor() {
                r.insert(
                    str_max.into(),
                    Arc::new(WildDocValue::Number(cursor.max.into())),
                );
                r.insert(
                    str_current.into(),
                    Arc::new(WildDocValue::Number(cursor.current.into())),
                );
            }
        }
        r
    }

    #[inline(always)]
    pub(super) fn session_gc(&self, vars: Vars) {
        self.database.write().session_gc(
            vars.get("expire")
                .and_then(|v| v.to_str().parse::<i64>().ok())
                .unwrap_or(60 * 60 * 24),
        );
    }
}
