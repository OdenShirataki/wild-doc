use std::{borrow::Cow, collections::HashMap, ops::Deref, sync::Arc};

#[derive(Debug)]
pub struct WildDocValue {
    value: serde_json::Value,
}
impl Deref for WildDocValue {
    type Target = serde_json::Value;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}
impl WildDocValue {
    pub fn new(value: serde_json::Value) -> Self {
        Self { value }
    }
    pub fn value(&self) -> &serde_json::Value {
        &self.value
    }
    pub fn to_str<'a>(&'a self) -> Cow<'a, str> {
        if let Some(s) = self.value.as_str() {
            Cow::Borrowed(s)
        } else {
            Cow::Owned(self.value.to_string())
        }
    }
}
pub type Vars = HashMap<Vec<u8>, Arc<WildDocValue>>;
pub type VarsStack = Vec<Vars>;
