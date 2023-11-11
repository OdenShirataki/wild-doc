use hashbrown::HashMap;
use maybe_xml::token::prop::Attributes;
use wild_doc_script::{Vars, WildDocValue};

use crate::xml_util;

use super::Parser;

impl Parser {
    pub(super) async fn output_attributes(&mut self, r: &mut Vec<u8>, attributes: Attributes<'_>) {
        for attr in attributes.into_iter() {
            if let (Ok(name), Some(value)) = (attr.name().to_str(), attr.value()) {
                if let Ok(value) = value.to_str() {
                    let (new_name, new_value) = self.attibute_var_or_script(name, value).await;
                    if new_name == "wd-attr:replace" {
                        if let Some(value) = new_value {
                            if !value.is_null() {
                                r.push(b' ');
                                r.extend(value.to_str().as_bytes());
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
                                    xml_util::escape_html(&value.to_str()).as_bytes(),
                                );
                            }
                        } else {
                            Self::output_attribute_value(r, value.as_bytes());
                        }
                    }
                }
            } else {
                r.extend(attr.as_bytes().to_vec());
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

        let mut values_per_script = HashMap::new();
        let mut futs_noscript = vec![];

        if let Some(attributes) = attributes {
            for attr in attributes.into_iter() {
                if let Ok(name) = attr.name().to_str() {
                    if let Some(value) = attr.value() {
                        if let Some(script_name) = Self::script_name(name) {
                            let new_name = unsafe {
                                std::str::from_utf8_unchecked(
                                    &name.as_bytes()[..name.len() - (script_name.len() + 1)],
                                )
                            };
                            let v = values_per_script.entry(script_name).or_insert(vec![]);
                            v.push((new_name, value));
                        } else {
                            if let Ok(value) = value.to_str() {
                                futs_noscript.push(async move {
                                    (name.into(), {
                                        let value = xml_util::quot_unescape(value);
                                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(
                                            value.as_str(),
                                        ) {
                                            json.into()
                                        } else {
                                            WildDocValue::String(value)
                                        }
                                    })
                                });
                            }
                        }
                    }
                }
            }
        }

        let mut futs = vec![];
        for (script_name, script) in self.scripts.iter_mut() {
            if let Some(v) = values_per_script.get(script_name.as_str()) {
                futs.push(async {
                    let mut r = Vars::new();
                    for (name, value) in v.into_iter() {
                        if let Ok(v) = script.eval(value.to_str().unwrap(), &self.stack).await {
                            r.insert(name.to_string(), v);
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
