use std::sync::Arc;

use serde_json::json;

use wild_doc_script::{Vars, WildDocValue};

use super::{Parser, SessionState};

impl Parser {
    #[must_use]
    pub(super) fn sessions(&self, vars: Vars) -> Vars {
        let mut r = Vars::new();

        if let Some(var) = vars.get(&self.strings.var) {
            let var = var.as_string();
            if var.as_str() != "" {
                let sessions = self.database.read().sessions();
                r.insert(var.into(), json!(sessions).into());
            }
        }
        r
    }

    #[must_use]
    pub(super) fn session(&self, vars: Vars) -> Option<SessionState> {
        if let Some(session_name) = vars.get(&self.strings.name) {
            let session_name = session_name.as_string();
            if session_name.as_str() != "" {
                let commit_on_close = vars
                    .get(&self.strings.commit_on_close)
                    .and_then(|v| v.as_bool())
                    .map_or(false, |v| *v);

                let clear_on_close = vars
                    .get(&self.strings.clear_on_close)
                    .and_then(|v| v.as_bool())
                    .map_or(false, |v| *v);

                let expire = vars
                    .get(&self.strings.expire)
                    .map_or_else(|| Arc::clone(&self.strings._blank), |v| v.as_string());
                let expire = if expire.len() > 0 {
                    expire.parse::<i64>().ok()
                } else {
                    None
                };
                let mut session = self.database.read().session(&session_name, expire);
                if let Some(cursor) = vars.get(&self.strings.cursor) {
                    let cursor = cursor.as_string();
                    if cursor.as_str() != "" {
                        if let Ok(cursor) = cursor.parse::<usize>() {
                            session.set_sequence_cursor(cursor)
                        }
                    }
                }
                if let Some(initialize) = vars.get(&self.strings.initialize) {
                    if initialize.as_bool().map_or(false, |v| *v) {
                        self.database.read().session_restart(&mut session, expire);
                    }
                }
                return Some(SessionState {
                    session,
                    commit_on_close,
                    clear_on_close,
                });
            }
        }
        None
    }

    #[must_use]
    pub(super) fn session_sequence(&self, vars: Vars) -> Vars {
        let mut str_max = vars
            .get(&self.strings.max)
            .map_or(Arc::clone(&self.strings._blank), |v| v.as_string());
        if str_max.as_str() == "" {
            str_max = Arc::clone(&self.strings.session_sequence_max);
        }

        let mut str_current = vars
            .get(&self.strings.current)
            .map_or(Arc::clone(&self.strings._blank), |v| v.as_string());
        if str_current.as_str() == "" {
            str_current = Arc::clone(&self.strings.session_sequence_current);
        }

        let mut r = Vars::new();
        if let Some(session_state) = self.sessions.last() {
            if let Some(cursor) = session_state.session.sequence_cursor() {
                r.insert(str_max.into(), WildDocValue::Number(cursor.max.into()));
                r.insert(
                    str_current.into(),
                    WildDocValue::Number(cursor.current.into()),
                );
            }
        }
        r
    }

    pub(super) fn session_gc(&self, vars: Vars) {
        self.database.write().session_gc(
            vars.get(&self.strings.expire)
                .and_then(|v| v.as_string().parse::<i64>().ok())
                .unwrap_or(60 * 60 * 24),
        );
    }
}
