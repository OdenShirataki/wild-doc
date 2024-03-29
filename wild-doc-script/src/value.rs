pub use semilattice_database_session::{SearchResult, SessionSearchResult};

use indexmap::IndexMap;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum WildDocValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(Arc<String>),
    Array(Vec<WildDocValue>),
    Object(Vars),
    Binary(Vec<u8>),
    SearchResult(Arc<SearchResult>),
    SessionSearchResult(Arc<SessionSearchResult>),
}
pub type Vars = IndexMap<Arc<String>, WildDocValue>;

impl Serialize for WildDocValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Bool(v) => v.serialize(serializer),
            Self::Number(v) => v.serialize(serializer),
            Self::String(v) => v.serialize(serializer),
            Self::Array(v) => v.serialize(serializer),
            Self::Object(v) => v.serialize(serializer),
            Self::Binary(v) => v.serialize(serializer),
            Self::SearchResult(_v) => "SearchResult".serialize(serializer), //(*v).serialize(serializer),
            Self::SessionSearchResult(_v) => "SessionSearchResult".serialize(serializer),
        }
    }
}

impl From<serde_json::Value> for WildDocValue {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(v) => Self::Bool(v),
            serde_json::Value::Number(v) => Self::Number(v),
            serde_json::Value::String(v) => Self::String(Arc::new(v)),
            serde_json::Value::Array(v) => {
                Self::Array(v.into_iter().map(|v| Self::from(v)).collect())
            }
            serde_json::Value::Object(v) => Self::Object(
                v.into_iter()
                    .map(|(k, v)| (Arc::new(k), Self::from(v)))
                    .collect(),
            ),
        }
    }
}
impl From<serde_json::Number> for WildDocValue {
    fn from(value: serde_json::Number) -> Self {
        Self::Number(value)
    }
}

impl std::fmt::Display for WildDocValue {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Null => {
                write!(f, "null")?;
                Ok(())
            }
            Self::Bool(v) => v.fmt(f),
            Self::Number(v) => v.fmt(f),
            Self::String(v) => v.fmt(f),
            Self::Array(v) => {
                write!(f, "[")?;
                let mut iter = v.into_iter();
                if let Some(i) = iter.next() {
                    i.fmt(f)?;
                }
                for i in iter {
                    write!(f, " , ")?;
                    i.fmt(f)?;
                }
                write!(f, "]")
            }
            Self::Object(v) => {
                write!(f, "{{")?;
                let mut iter = v.into_iter();
                if let Some((k, v)) = iter.next() {
                    write!(f, "\"{}\" : ", k)?;
                    v.fmt(f)?;
                }
                for (k, v) in iter {
                    write!(f, " , \"{}\" : ", k)?;
                    v.fmt(f)?;
                }
                write!(f, "}}")
            }
            Self::Binary(v) => {
                write!(f, "{:?}", v)
            }
            Self::SearchResult(v) => {
                write!(f, "{:?}", v)
            }
            Self::SessionSearchResult(v) => {
                write!(f, "{:?}", v)
            }
        }
    }
}

impl WildDocValue {
    #[inline(always)]
    pub fn as_string(&self) -> Arc<String> {
        match self {
            Self::String(s) => Arc::clone(s),
            Self::Binary(value) => Arc::new(unsafe { std::str::from_utf8_unchecked(value) }.into()),
            _ => Arc::new(self.to_string()),
        }
    }

    #[inline(always)]
    pub fn is_object(&self) -> bool {
        match self {
            Self::Object(_) => true,
            _ => false,
        }
    }

    #[inline(always)]
    pub fn is_null(&self) -> bool {
        match self {
            Self::Null => true,
            _ => false,
        }
    }

    #[inline(always)]
    pub fn as_bool(&self) -> Option<&bool> {
        match self {
            Self::Bool(v) => Some(v),
            _ => None,
        }
    }
}
