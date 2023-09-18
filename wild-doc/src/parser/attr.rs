use std::{collections::HashMap, sync::Arc};

use bson::Bson;
use maybe_xml::token::prop::Attributes;

use crate::xml_util;

use super::{AttributeMap, Parser};

impl Parser {
    #[inline(always)]
    pub(super) fn output_attributes(&mut self, r: &mut Vec<u8>, attributes: Attributes) {
        attributes.iter().for_each(|attr| {
            let name = attr.name();
            if let Some(value) = attr.value() {
                let (new_name, new_value) =
                    self.attibute_var_or_script(name.as_bytes(), value.as_bytes());
                if new_name == b"wd-attr:replace" {
                    if let Some(value) = new_value {
                        r.push(b' ');
                        r.extend(
                            if let Some(value) = value.as_str() {
                                value.to_owned()
                            } else {
                                value.to_string()
                            }
                            .into_bytes(),
                        );
                    }
                } else {
                    r.push(b' ');
                    r.extend(new_name.to_vec());
                    if let Some(value) = new_value {
                        Self::output_attribute_value(
                            r,
                            xml_util::escape_html(&if let Some(value) = value.as_str() {
                                value.to_owned()
                            } else {
                                value.to_string()
                            })
                            .as_bytes(),
                        );
                    } else {
                        Self::output_attribute_value(r, value.as_bytes());
                    }
                }
            } else {
                r.extend(attr.to_vec());
            };
        });
    }

    #[inline(always)]
    pub(super) fn parse_attibutes(&mut self, attributes: &Option<Attributes>) -> AttributeMap {
        let mut r: AttributeMap = HashMap::new();
        if let Some(attributes) = attributes {
            attributes.iter().for_each(|attr| {
                if let Some(value) = attr.value() {
                    if let (prefix, Some(value)) =
                        self.attibute_var_or_script(attr.name().as_bytes(), value.as_bytes())
                    {
                        r.insert(prefix.to_vec(), Some(Arc::new(value)));
                    } else {
                        r.insert(attr.name().to_vec(), {
                            let value = xml_util::quot_unescape(value.as_bytes());
                            Some(Arc::new(match value.as_str() {
                                "true" => Bson::from(true),
                                "false" => Bson::from(false),
                                _ => Bson::from(value),
                            }))
                        });
                    }
                } else {
                    r.insert(attr.name().to_vec(), None);
                }
            });
        }
        r
    }

    #[inline(always)]
    fn attribute_script(&mut self, script: &str, value: &[u8]) -> Option<Bson> {
        self.scripts.get(script).and_then(|script| {
            script
                .lock()
                .unwrap()
                .eval(xml_util::quot_unescape(value).as_bytes())
                .ok()
        })
    }

    #[inline(always)]
    fn output_attribute_value(r: &mut Vec<u8>, val: &[u8]) {
        r.push(b'=');
        r.push(b'"');
        r.extend(val.to_vec());
        r.push(b'"');
    }

    #[inline(always)]
    fn attibute_var_or_script<'a>(
        &'a mut self,
        name: &'a [u8],
        value: &[u8],
    ) -> (&[u8], Option<Bson>) {
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
