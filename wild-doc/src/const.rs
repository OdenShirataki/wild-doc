use once_cell::sync::Lazy;
use std::sync::Arc;

macro_rules! def {
    ($var:ident,$val:expr) => {
        pub const $var: Lazy<Arc<String>> = Lazy::new(|| Arc::new($val.into()));
    };
}

def!(_BLANK, "");
def!(ACTIVITY, "activity");
def!(BASE64, "base64");
def!(CLEAR_ON_CLOSE, "clear_on_close");
def!(COLLECTION, "collection");
def!(COLLECTION_ID, "collection_id");
def!(COLLECTION_NAME, "collection_name");
def!(COMMIT, "commit");
def!(COMMIT_ON_CLOSE, "commit_on_close");
def!(COMMIT_ROWS, "commit_rows");
def!(CONTINUE, "continue");
def!(CURRENT, "current");
def!(CURSOR, "cursor");
def!(
    CREATE_COLLECTION_IF_NOT_EXISTS,
    "create_collection_if_not_exists"
);
def!(DELETE, "delete");
def!(DEPENDS, "depends");
def!(EXPIRE, "expire");
def!(FIELD, "field");
def!(FIELDS, "fields");
def!(IN, "in");
def!(INHERIT_DEPEND_IF_EMPTY, "inherit_depend_if_empty");
def!(INITIALIZE, "initialize");
def!(KEY, "key");
def!(LAST_UPDATED, "last_updated");
def!(MAX, "max");
def!(METHOD, "method");
def!(NAME, "name");
def!(ORDER, "order");
def!(RELATION, "relation");
def!(RESULT, "result");
def!(ROW, "row");
def!(SERIAL, "serial");
def!(SESSION_ROWS, "session_rows");
def!(SESSION_SEQUENCE_CURRENT, "session_sequence_current");
def!(SESSION_SEQUENCE_MAX, "session_sequence_max");
def!(SRC, "src");
def!(SUBSTITUTE, "substitute");
def!(TERM, "term");
def!(TERM_BEGIN, "term_begin");
def!(TERM_END, "term_end");
def!(UPDATE, "update");
def!(UUID, "uuid");
def!(VAR, "var");
def!(VALUE, "value");
def!(WITHOUT_SESSION, "without_session");
