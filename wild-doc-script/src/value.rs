use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, RwLock},
};

use indexmap::IndexMap;
use serde::Serialize;

#[derive(Debug, Clone)]
pub enum WildDocValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<WildDocValue>),
    Object(IndexMap<String, WildDocValue>),
    Binary(Vec<u8>),
}

impl Serialize for WildDocValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            WildDocValue::Null => serializer.serialize_none(),
            WildDocValue::Bool(v) => v.serialize(serializer),
            WildDocValue::Number(v) => v.serialize(serializer),
            WildDocValue::String(v) => v.serialize(serializer),
            WildDocValue::Array(v) => v.serialize(serializer),
            WildDocValue::Object(v) => v.serialize(serializer),
            WildDocValue::Binary(v) => v.serialize(serializer),
        }
    }
}

impl From<serde_json::Value> for WildDocValue {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(v) => Self::Bool(v),
            serde_json::Value::Number(v) => Self::Number(v),
            serde_json::Value::String(v) => Self::String(v),
            serde_json::Value::Array(v) => {
                Self::Array(v.iter().map(|v| Self::from(v.clone())).collect())
            }
            serde_json::Value::Object(v) => Self::Object(
                v.iter()
                    .map(|(k, v)| (k.to_owned(), Self::from(v.clone())))
                    .collect(),
            ),
        }
    }
}
impl From<serde_json::Number> for WildDocValue {
    fn from(value: serde_json::Number) -> Self {
        WildDocValue::Number(value)
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
                let mut iter = v.iter();
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
                let mut iter = v.iter();
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
        }
    }
}

impl WildDocValue {
    pub fn to_str(&self) -> Cow<str> {
        match self {
            Self::String(s) => Cow::Borrowed(s),
            Self::Binary(value) => Cow::Borrowed(unsafe { std::str::from_utf8_unchecked(value) }),
            _ => Cow::Owned(self.to_string()),
        }
    }
    pub fn is_object(&self) -> bool {
        match self {
            Self::Object(_) => true,
            _ => false,
        }
    }
    pub fn is_null(&self) -> bool {
        match self {
            Self::Null => true,
            _ => false,
        }
    }
    pub fn as_bool(&self) -> Option<&bool> {
        match self {
            Self::Bool(v) => Some(v),
            _ => None,
        }
    }
}
pub type Vars = HashMap<Vec<u8>, Arc<RwLock<WildDocValue>>>;
pub type VarsStack = Vec<Vars>;
