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
                let name = name.as_bytes();
                let value = value.as_bytes();
                let (new_name, new_value) = self.attibute_var_or_script(name, value).await;
                if new_name == b"wd-attr:replace" {
                    if let Some(value) = new_value {
                        if !value.is_null() {
                            r.push(b' ');
                            r.extend(value.to_str().as_bytes());
                        }
                    }
                } else {
                    r.push(b' ');
                    r.extend(new_name);
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
                        Self::output_attribute_value(r, value);
                    }
                }
            } else {
                r.extend(attr.as_bytes().to_vec());
            };
        }
    }

    pub(super) async fn parse_attibutes(
        &mut self,
        attributes: Option<Attributes<'_>>,
    ) -> AttributeMap {
        let mut r: AttributeMap = HashMap::new();
        if let Some(attributes) = attributes {
            for attr in attributes.into_iter() {
                let name = attr.name().as_bytes();
                if let Some(value) = attr.value() {
                    let org_value = value.as_bytes();
                    let (name, value) = self.attibute_var_or_script(name, org_value).await;
                    //TODO: Consider, to future per script type
                    r.insert(
                        name.to_vec(),
                        Some(Arc::new(if let Some(value) = value {
                            value
                        } else {
                            let value = xml_util::quot_unescape(org_value);
                            if let Ok(json) =
                                serde_json::from_str::<serde_json::Value>(value.as_str())
                            {
                                json.into()
                            } else {
                                WildDocValue::String(value)
                            }
                        })),
                    );
                } else {
                    r.insert(name.to_vec(), None);
                }
            }
        }
        r
    }

    #[inline(always)]
    fn output_attribute_value(r: &mut Vec<u8>, val: &[u8]) {
        r.extend(b"=\"");
        r.extend(val);
        r.push(b'"');
    }

    async fn attibute_var_or_script<'a>(
        &mut self,
        name: &'a [u8],
        value: &'a [u8],
    ) -> (&'a [u8], Option<WildDocValue>) {
        let mut splited = name.split(|p| *p == b':').collect::<Vec<&[u8]>>();
        if splited.len() >= 2 {
            let script = unsafe { std::str::from_utf8_unchecked(splited.pop().unwrap()) };
            (
                &name[..name.len() - (script.len() + 1)],
                if let Some(script) = self.scripts.get_mut(script) {
                    script
                        .eval(xml_util::quot_unescape(value).as_bytes())
                        .await
                        .ok()
                } else {
                    None
                },
            )
        } else {
            (name, None)
        }
    }
}
