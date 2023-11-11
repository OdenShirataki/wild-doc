use anyhow::Result;

use maybe_xml::token::prop::Attributes;
use wild_doc_script::Vars;

use super::{Parser, WildDocValue};

impl Parser {
    pub(super) async fn r#for(&mut self, attr: Vars, xml: &[u8]) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        if let (Some(var), Some(r#in)) = (attr.get("var"), attr.get("in")) {
            let var = var.to_str();
            if var != "" {
                match r#in {
                    WildDocValue::Object(map) => {
                        if let Some(key_name) = attr.get("key") {
                            for (key, value) in map.into_iter() {
                                let mut new_vars = Vars::new();
                                new_vars.insert(var.to_string(), value.clone());
                                new_vars.insert(
                                    key_name.to_str().into(),
                                    serde_json::json!(key).into(),
                                );
                                let mut pos = 0;
                                self.stack.push(new_vars);
                                r.extend(self.parse(xml, &mut pos).await?);
                                self.stack.pop();
                            }
                        } else {
                            for (_, value) in map.into_iter() {
                                let mut new_vars = Vars::new();
                                new_vars.insert(var.to_string(), value.clone());
                                let mut pos = 0;
                                self.stack.push(new_vars);
                                r.extend(self.parse(xml, &mut pos).await?);
                                self.stack.pop();
                            }
                        }
                    }
                    WildDocValue::Array(vec) => {
                        let key_name = attr.get("key");
                        if let Some(key_name) = key_name {
                            let mut key = 0;
                            for value in vec.into_iter() {
                                key += 1;
                                let mut new_vars = Vars::new();
                                new_vars.insert(var.to_string(), value.clone());
                                new_vars.insert(
                                    key_name.to_str().into(),
                                    serde_json::json!(key).into(),
                                );
                                let mut pos = 0;
                                self.stack.push(new_vars);
                                r.extend(self.parse(xml, &mut pos).await?);
                                self.stack.pop();
                            }
                        } else {
                            for value in vec.into_iter() {
                                let mut new_vars = Vars::new();
                                new_vars.insert(var.to_string(), value.clone());
                                let mut pos = 0;
                                self.stack.push(new_vars);
                                r.extend(self.parse(xml, &mut pos).await?);
                                self.stack.pop();
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(r)
    }

    pub(super) async fn r#while(
        &mut self,
        attributes: Option<Attributes<'_>>,
        xml: &[u8],
    ) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        loop {
            if self
                .vars_from_attibutes(attributes)
                .await
                .get("continue")
                .and_then(|v| v.as_bool())
                .map_or(false, |v| *v)
            {
                let mut pos = 0;
                r.extend(self.parse(xml, &mut pos).await?);
            } else {
                break;
            }
        }
        Ok(r)
    }
}
