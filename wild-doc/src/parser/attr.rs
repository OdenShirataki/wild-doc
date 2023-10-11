use std::sync::Arc;

use hashbrown::HashMap;
use maybe_xml::token::prop::Attributes;
use wild_doc_script::WildDocValue;

use crate::xml_util;

use super::{AttributeMap, Parser};

impl Parser {
    pub(super) async fn output_attributes(&mut self, r: &mut Vec<u8>, attributes: Attributes<'_>) {
        for attr in attributes {
            let name = attr.name();
            if let Some(value) = attr.value() {
                let (new_name, new_value) = self
                    .attibute_var_or_script(name.as_bytes(), value.as_bytes())
                    .await;
                if new_name == b"wd-attr:replace" {
                    if let Some(value) = new_value {
                        if !value.is_null() {
                            r.push(b' ');
                            r.extend(value.to_str().to_string().into_bytes());
                        }
                    }
                } else {
                    r.push(b' ');
                    r.extend(new_name.to_vec());
                    if let Some(value) = new_value {
                        if value.is_null() {
                            Self::output_attribute_value(r, b"");
                        } else {
                            Self::output_attribute_value(
                                r,
                                xml_util::escape_html(value.to_str().as_ref()).as_bytes(),
                            );
                        }
                    } else {
                        Self::output_attribute_value(r, value.as_bytes());
                    }
                }
            } else {
                r.extend(attr.as_bytes());
            };
        }
    }

    pub(super) async fn parse_attibutes(
        &mut self,
        attributes: &Option<Attributes<'_>>,
    ) -> AttributeMap {
        let mut r: AttributeMap = HashMap::new();
        if let Some(attributes) = attributes {
            for attr in attributes.into_iter() {
                if let Some(value) = attr.value() {
                    if let (prefix, Some(value)) = self
                        .attibute_var_or_script(attr.name().as_bytes(), value.as_bytes())
                        .await
                    {
                        r.insert(prefix.to_vec(), Some(Arc::new(value)));
                    } else {
                        r.insert(attr.name().as_bytes().to_vec(), {
                            let value = xml_util::quot_unescape(value.as_bytes());
                            let json = serde_json::from_str::<serde_json::Value>(value.as_str());
                            Some(Arc::new(if let Ok(json) = json {
                                WildDocValue::from(json)
                            } else {
                                WildDocValue::String(value)
                            }))
                        });
                    }
                } else {
                    r.insert(attr.name().as_bytes().to_vec(), None);
                }
            }
        }
        r
    }

    async fn attribute_script(&mut self, script: &str, value: &[u8]) -> Option<WildDocValue> {
        if let Some(script) = self.scripts.get(script) {
            script
                .eval(xml_util::quot_unescape(value).as_bytes())
                .await
                .ok()
        } else {
            None
        }
    }

    #[inline(always)]
    fn output_attribute_value(r: &mut Vec<u8>, val: &[u8]) {
        r.push(b'=');
        r.push(b'"');
        r.extend(val.to_vec());
        r.push(b'"');
    }

    async fn attibute_var_or_script<'a>(
        &mut self,
        name: &'a [u8],
        value: &[u8],
    ) -> (&'a [u8], Option<WildDocValue>) {
        let splited = name.split(|p| *p == b':').collect::<Vec<&[u8]>>();
        if let (Some(name), Some(script)) = (splited.get(0), splited.get(1)) {
            let script = unsafe { std::str::from_utf8_unchecked(script) };
            (*name, self.attribute_script(script, value).await)
        } else {
            (name, None)
        }
    }
}
