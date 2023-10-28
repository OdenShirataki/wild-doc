use std::sync::Arc;

use hashbrown::HashMap;
use maybe_xml::token::prop::Attributes;
use wild_doc_script::WildDocValue;

use crate::xml_util;

use super::{AttributeMap, Parser};

impl Parser {
    pub(super) async fn output_attributes(&mut self, r: &mut Vec<u8>, attributes: Attributes<'_>) {
        for attr in attributes.into_iter() {
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
                                xml_util::escape_html(&value.to_str()).as_bytes(),
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

    fn script_name(name: &[u8]) -> Option<&str> {
        let mut splited = name.split(|p| *p == b':').collect::<Vec<&[u8]>>();
        if splited.len() >= 2 {
            Some(unsafe { std::str::from_utf8_unchecked(splited.pop().unwrap()) })
        } else {
            None
        }
    }
    pub(super) async fn parse_attibutes(
        &mut self,
        attributes: Option<Attributes<'_>>,
    ) -> AttributeMap {
        let mut r = AttributeMap::new();

        let mut values_per_script = HashMap::new();
        let mut futs_noscript = vec![];

        if let Some(attributes) = attributes {
            for attr in attributes.into_iter() {
                let name = attr.name();
                if let Ok(str_name) = name.to_str() {
                    if let Some(value) = attr.value() {
                        let name_bytes = name.as_bytes();
                        if let Some(script_name) = Self::script_name(name_bytes) {
                            let new_name = unsafe {
                                std::str::from_utf8_unchecked(
                                    &name_bytes[..name_bytes.len() - (script_name.len() + 1)],
                                )
                            };
                            let v = values_per_script.entry(script_name).or_insert(vec![]);
                            v.push((new_name, value));
                        } else {
                            futs_noscript.push(async move {
                                (
                                    str_name.into(),
                                    Some(Arc::new({
                                        let value = xml_util::quot_unescape(value.as_bytes());
                                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(
                                            value.as_str(),
                                        ) {
                                            json.into()
                                        } else {
                                            WildDocValue::String(value)
                                        }
                                    })),
                                )
                            });
                        }
                    } else {
                        r.insert(str_name.into(), None);
                    }
                }
            }
        }

        let mut futs = vec![];
        for (script_name, script) in self.scripts.iter_mut() {
            if let Some(v) = values_per_script.get(script_name.as_str()) {
                futs.push(async move {
                    let mut r = AttributeMap::new();
                    for (name, value) in v.into_iter() {
                        if let Ok(v) = script.eval(value.as_bytes()).await {
                            r.insert(name.to_string(), Some(v));
                        }
                    }
                    r
                })
            }
        }

        let (f1, f2) = futures::future::join(
            futures::future::join_all(futs),
            futures::future::join_all(futs_noscript),
        )
        .await;
        r.extend(f1.into_iter().flatten());
        r.extend(f2.into_iter());
        r
    }

    #[inline(always)]
    fn output_attribute_value(r: &mut Vec<u8>, val: &[u8]) {
        r.extend(b"=\"");
        r.extend(val);
        r.push(b'"');
    }

    pub(crate) async fn attibute_var_or_script<'a>(
        &mut self,
        name: &'a [u8],
        value: &'a [u8],
    ) -> (&'a [u8], Option<Arc<WildDocValue>>) {
        if let Some(script_name) = Self::script_name(name) {
            (
                &name[..name.len() - (script_name.len() + 1)],
                if let Some(script) = self.scripts.get_mut(script_name) {
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
