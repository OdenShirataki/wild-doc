use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, RwLock},
};

#[derive(Debug, Clone)]
pub enum WildDocValue {
    Json(serde_json::Value),
    Binary(Vec<u8>),
}
impl From<serde_json::Value> for WildDocValue {
    fn from(value: serde_json::Value) -> Self {
        WildDocValue::Json(value)
    }
}
impl From<Vec<u8>> for WildDocValue {
    fn from(value: Vec<u8>) -> Self {
        WildDocValue::Binary(value)
    }
}

impl std::fmt::Display for WildDocValue {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Json(json) => match json {
                serde_json::Value::String(s) => {
                    write!(f, "{}", s)?;
                    Ok(())
                }
                _ => json.fmt(f),
            },
            Self::Binary(v) => {
                write!(f, "{}", std::str::from_utf8(v).map_or_else(|_| "", |v| v))?;
                Ok(())
            }
        }
    }
}

impl WildDocValue {
    pub fn to_json_value(&self) -> Cow<serde_json::Value> {
        match self {
            Self::Json(json) => Cow::Borrowed(json),
            Self::Binary(value) => Cow::Owned(
                serde_json::from_slice(value).map_or_else(|_| serde_json::json!({}), |v| v),
            ),
        }
    }
    pub fn to_json_value_mut(&mut self) -> Option<&mut serde_json::Value> {
        match self {
            Self::Json(json) => Some(json),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Cow<[u8]> {
        match self {
            Self::Json(json) => Cow::Owned(json.to_string().into_bytes()),
            Self::Binary(value) => Cow::Borrowed(value),
        }
    }
    pub fn to_str(&self) -> Cow<str> {
        match self {
            Self::Json(json) => Cow::Owned(json.to_string()),
            Self::Binary(value) => Cow::Borrowed(unsafe { std::str::from_utf8_unchecked(value) }),
        }
    }
}
pub type Vars = HashMap<Vec<u8>, Arc<RwLock<WildDocValue>>>;
pub type VarsStack = Vec<Vars>;
