use std::sync::Arc;

use futures::FutureExt;
use hashbrown::HashMap;
use maybe_xml::token::prop::Attributes;
use wild_doc_script::WildDocValue;

use crate::xml_util;

use super::{AttributeMap, Parser};

impl Parser {
    pub(super) async fn output_attributes(&mut self, r: &mut Vec<u8>, attributes: Attributes<'_>) {
        let mut futs = vec![];
        for attr in attributes {
            let name = attr.name();
            if let Some(value) = attr.value() {
                let name = name.as_bytes();
                let value = value.as_bytes();
                futs.push(
                    async {
                        let mut r = vec![];
                        let (new_name, new_value, _org_value) =
                            self.attibute_var_or_script(name, value).await;
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
                                    Self::output_attribute_value(&mut r, b"");
                                } else {
                                    Self::output_attribute_value(
                                        &mut r,
                                        xml_util::escape_html(value.to_str().as_ref()).as_bytes(),
                                    );
                                }
                            } else {
                                Self::output_attribute_value(&mut r, value);
                            }
                        }
                        r
                    }
                    .boxed_local(),
                );
            } else {
                futs.push(async move { attr.as_bytes().to_vec() }.boxed_local());
            };
        }
        r.extend(futures::future::join_all(futs).await.concat());
    }

    pub(super) async fn parse_attibutes(&self, attributes: Option<Attributes<'_>>) -> AttributeMap {
        let mut r: AttributeMap = HashMap::new();
        let mut futs = vec![];
        if let Some(attributes) = attributes {
            for attr in attributes.into_iter() {
                let name = attr.name().as_bytes();
                if let Some(value) = attr.value() {
                    futs.push(self.attibute_var_or_script(name, value.as_bytes()));
                } else {
                    r.insert(name.to_vec(), None);
                }
            }
        }

        r.extend(futures::future::join_all(futs).await.into_iter().map(
            |(name, value, org_value)| {
                (
                    name.to_vec(),
                    Some(Arc::new(if let Some(value) = value {
                        value
                    } else {
                        let value = xml_util::quot_unescape(org_value);
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(value.as_str())
                        {
                            WildDocValue::from(json)
                        } else {
                            WildDocValue::String(value)
                        }
                    })),
                )
            },
        ));
        r
    }

    async fn attribute_script(&self, script: &str, value: &[u8]) -> Option<WildDocValue> {
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
        r.extend(b"=\"");
        r.extend(val.to_vec());
        r.push(b'"');
    }

    async fn attibute_var_or_script<'a>(
        &self,
        name: &'a [u8],
        value: &'a [u8],
    ) -> (&'a [u8], Option<WildDocValue>, &'a [u8]) {
        let mut splited = name.split(|p| *p == b':').collect::<Vec<&[u8]>>();
        if splited.len() >= 2 {
            let script = splited.pop().unwrap();
            (
                &name[..name.len() - (script.len() + 1)],
                self.attribute_script(unsafe { std::str::from_utf8_unchecked(script) }, value)
                    .await,
                value,
            )
        } else {
            (name, None, value)
        }
    }
}
