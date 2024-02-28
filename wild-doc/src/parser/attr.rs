use std::sync::Arc;

use maybe_xml::token::prop::Attributes;
use wild_doc_script::{IncludeAdaptor, Vars, WildDocValue};

use crate::xml_util;

use super::Parser;

impl<I: IncludeAdaptor + Send> Parser<I> {
    pub(super) async fn output_attributes(&mut self, r: &mut Vec<u8>, attributes: Attributes<'_>) {
        for attr in attributes.into_iter() {
            let name = attr.name().as_str();
            if let Some(value) = attr.value() {
                let value = value.as_str();
                let (new_name, new_value) = self.attibute_var_or_script(name, value).await;
                if new_name == "wd:attr" {
                    if let Some(value) = new_value {
                        if !value.is_null() {
                            r.push(b' ');
                            r.extend(value.as_string().as_bytes());
                        }
                    }
                } else {
                    r.push(b' ');
                    r.extend(new_name.as_bytes());
                    if let Some(value) = new_value {
                        if value.is_null() {
                            Self::output_attribute_value(r, b"");
                        } else {
                            Self::output_attribute_value(
                                r,
                                xml_util::escape_html(&value.as_string()).as_bytes(),
                            );
                        }
                    } else {
                        Self::output_attribute_value(r, value.as_bytes());
                    }
                }
            } else {
                r.push(b' ');
                r.extend(name.as_bytes().to_vec());
            };
        }
    }

    fn script_name(name: &str) -> Option<&str> {
        let mut splited: Vec<_> = name.split(':').collect();
        if splited.len() >= 2 {
            Some(splited.pop().unwrap())
        } else {
            None
        }
    }

    #[must_use]
    pub(super) async fn vars_from_attibutes(&mut self, attributes: Option<Attributes<'_>>) -> Vars {
        let mut r = Vars::new();

        if let Some(attributes) = attributes {
            for attr in attributes.into_iter() {
                if let Some(value) = attr.value() {
                    let name = attr.name().as_str();
                    if let Some(script_name) = Self::script_name(name) {
                        if let Some(script) = self.scripts.get_mut(script_name) {
                            if let Ok(v) = script.eval(value.as_str(), &self.stack).await {
                                let name = unsafe {
                                    std::str::from_utf8_unchecked(
                                        &name.as_bytes()[..name.len() - (script_name.len() + 1)],
                                    )
                                };
                                r.insert(Arc::new(name.to_string()), v);
                            }
                        }
                    } else {
                        let value = value.as_str();
                        r.insert(Arc::new(name.into()), {
                            let value = xml_util::quot_unescape(value);
                            if let Ok(json) =
                                serde_json::from_str::<serde_json::Value>(value.as_str())
                            {
                                json.into()
                            } else {
                                WildDocValue::String(Arc::new(value))
                            }
                        });
                    }
                }
            }
        }

        r
    }

    fn output_attribute_value(r: &mut Vec<u8>, val: &[u8]) {
        r.extend(b"=\"");
        r.extend(val);
        r.push(b'"');
    }

    pub(crate) async fn attibute_var_or_script<'a>(
        &mut self,
        name: &'a str,
        value: &str,
    ) -> (&'a str, Option<WildDocValue>) {
        if let Some(script_name) = Self::script_name(name) {
            (
                unsafe {
                    std::str::from_utf8_unchecked(
                        &name.as_bytes()[..name.len() - (script_name.len() + 1)],
                    )
                },
                if let Some(script) = self.scripts.get_mut(script_name) {
                    script
                        .eval(&xml_util::quot_unescape(value), &self.stack)
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
