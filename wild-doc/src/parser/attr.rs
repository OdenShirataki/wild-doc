use std::{collections::HashMap, sync::Arc};

use maybe_xml::token::prop::Attributes;
use wild_doc_script::WildDocValue;

use crate::xml_util;

use super::{AttributeMap, Parser};

impl Parser {
    pub(super) fn output_attributes(&mut self, r: &mut Vec<u8>, attributes: Attributes) {
        for attribute in attributes {
            let name = attribute.name();
            if let Some(value) = attribute.value() {
                let (new_name, new_value) =
                    self.attibute_var_or_script(name.as_bytes(), value.as_bytes());
                if new_name == b"wd-attr:replace" {
                    if let Some(value) = new_value {
                        r.push(b' ');
                        r.extend(value.to_str().as_bytes().to_vec());
                    }
                } else {
                    r.push(b' ');
                    r.extend(new_name.to_vec());
                    if let Some(value) = new_value {
                        Self::output_attribute_value(
                            r,
                            xml_util::escape_html(&value.to_str()).as_bytes(),
                        );
                    } else {
                        Self::output_attribute_value(r, value.as_bytes());
                    }
                }
            } else {
                r.extend(attribute.to_vec());
            };
        }
    }

    pub(super) fn parse_attibutes(&mut self, attributes: &Option<Attributes>) -> AttributeMap {
        let mut r: AttributeMap = HashMap::new();
        if let Some(attributes) = attributes {
            for attribute in attributes.iter() {
                if let Some(value) = attribute.value() {
                    if let (prefix, Some(value)) =
                        self.attibute_var_or_script(attribute.name().as_bytes(), value.as_bytes())
                    {
                        r.insert(prefix.to_vec(), Some(Arc::new(value)));
                    } else {
                        r.insert(attribute.name().to_vec(), {
                            let value = xml_util::quot_unescape(value.as_bytes());
                            Some(Arc::new(WildDocValue::new(
                                if let Ok(json_value) = serde_json::from_str(value.as_str()) {
                                    json_value
                                } else {
                                    serde_json::json!(value.as_str())
                                },
                            )))
                        });
                    }
                } else {
                    r.insert(attribute.name().to_vec(), None);
                }
            }
        }
        r
    }

    fn attribute_script<'a>(&mut self, script: &str, value: &[u8]) -> Option<WildDocValue> {
        self.scripts.get(script).and_then(|script| {
            script
                .lock()
                .unwrap()
                .eval(xml_util::quot_unescape(value).as_bytes())
                .ok()
                .map(|v| WildDocValue::new(v))
        })
    }
    fn output_attribute_value(r: &mut Vec<u8>, val: &[u8]) {
        r.push(b'=');
        r.push(b'"');
        r.extend(val.to_vec());
        r.push(b'"');
    }

    fn attibute_var_or_script<'a>(
        &'a mut self,
        name: &'a [u8],
        value: &[u8],
    ) -> (&[u8], Option<WildDocValue>) {
        for key in self.scripts.keys() {
            if name.ends_with((":".to_owned() + key.as_str()).as_bytes()) {
                return (
                    &name[..name.len() - (key.len() + 1)],
                    self.attribute_script(key.to_owned().as_str(), value),
                );
            }
        }
        (name, None)
    }
}
