use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use anyhow::Result;

use maybe_xml::{
    scanner::{Scanner, State},
    token::{self, prop::Attributes},
};

use crate::xml_util;

use super::{AttributeMap, Parser, WildDocValue};

impl Parser {
    pub(super) fn get_include_content(&mut self, attributes: AttributeMap) -> Result<Vec<u8>> {
        if let Some(Some(src)) = attributes.get(b"src".as_ref()) {
            let src = src.to_str().into_owned();
            let (xml, filename) = if let Some(xml) = self
                .state
                .include_adaptor()
                .lock()
                .unwrap()
                .include(src.clone().into())
            {
                (Some(xml), src.into())
            } else {
                let mut r = (None, "".to_owned());
                if let Some(Some(substitute)) = attributes.get(b"substitute".as_ref()) {
                    let substitute = substitute.to_str().into_owned();
                    if let Some(xml) = self
                        .state
                        .include_adaptor()
                        .lock()
                        .unwrap()
                        .include(substitute.clone().into())
                    {
                        r = (Some(xml), substitute.into());
                    }
                }
                r
            };
            if let Some(xml) = xml {
                if xml.len() > 0 {
                    self.include_stack.push(filename);
                    let r = self.parse(xml.as_slice())?;
                    self.include_stack.pop();
                    return Ok(r);
                }
            }
        }
        Ok(b"".to_vec())
    }

    pub(super) fn case(&mut self, attributes: AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        let cmp_src = if let Some(Some(value)) = attributes.get(b"value".as_ref()) {
            value.to_str()
        } else {
            "".into()
        };
        let mut xml = xml;
        let mut scanner = Scanner::new();
        while let Some(state) = scanner.scan(&xml) {
            match state {
                State::ScannedStartTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::StartTag::from(token_bytes);
                    let name = token.name();

                    match name.as_bytes() {
                        b"wd:when" => {
                            let (inner_xml, outer_end) = xml_util::inner(xml);
                            xml = &xml[outer_end..];
                            let attributes = self.parse_attibutes(&token.attributes());
                            if let Some(Some(right)) = attributes.get(b"value".as_ref()) {
                                if cmp_src == right.to_str() {
                                    return Ok(self.parse(inner_xml)?);
                                }
                            }
                        }
                        b"wd:else" => {
                            let (inner_xml, _) = xml_util::inner(xml);
                            return Ok(self.parse(inner_xml)?);
                        }
                        _ => {}
                    }
                }
                State::ScannedEmptyElementTag(pos)
                | State::ScannedEndTag(pos)
                | State::ScannedCharacters(pos)
                | State::ScannedCdata(pos)
                | State::ScannedComment(pos)
                | State::ScannedDeclaration(pos) => {
                    xml = &xml[pos..];
                }
                _ => {
                    break;
                }
            }
        }
        Ok(vec![])
    }

    pub(super) fn r#if(&mut self, attributes: AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        if let Some(Some(value)) = attributes.get(b"value".as_ref()) {
            if value.to_str() == "true" {
                return self.parse(xml);
            }
        }
        Ok(vec![])
    }

    pub(super) fn r#for(&mut self, attributes: AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        if let (Some(Some(var)), Some(Some(r#in))) = (
            attributes.get(b"var".as_ref()),
            attributes.get(b"in".as_ref()),
        ) {
            let var = var.to_str();
            if var != "" {
                match &r#in.value() {
                    serde_json::Value::Object(map) => {
                        let key_name = attributes.get(b"key".as_ref());
                        for (key, value) in map {
                            let mut vars = HashMap::new();
                            vars.insert(
                                var.as_bytes().to_vec(),
                                Arc::new(RwLock::new(WildDocValue::new(value.clone()))),
                            );
                            if let Some(Some(key_name)) = key_name {
                                vars.insert(
                                    key_name.to_str().as_bytes().to_vec(),
                                    Arc::new(RwLock::new(WildDocValue::new(serde_json::json!(
                                        key
                                    )))),
                                );
                            }
                            self.state.stack().write().unwrap().push(vars);
                            r.extend(self.parse(xml)?);
                            self.state.stack().write().unwrap().pop();
                        }
                    }
                    serde_json::Value::Array(vec) => {
                        let key_name = attributes.get(b"key".as_ref());
                        let mut key = 0;
                        for value in vec {
                            let mut vars = HashMap::new();
                            vars.insert(
                                var.as_bytes().to_vec(),
                                Arc::new(RwLock::new(WildDocValue::new(value.clone()))),
                            );
                            if let Some(Some(key_name)) = key_name {
                                vars.insert(
                                    key_name.to_str().as_bytes().to_vec(),
                                    Arc::new(RwLock::new(WildDocValue::new(serde_json::json!(
                                        key
                                    )))),
                                );
                                key += 1;
                            }
                            self.state.stack().write().unwrap().push(vars);
                            r.extend(self.parse(xml)?);
                            self.state.stack().write().unwrap().pop();
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(r)
    }
    pub(super) fn r#while(
        &mut self,
        attributes: Option<Attributes<'_>>,
        xml: &[u8],
    ) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        loop {
            let attributes = self.parse_attibutes(&attributes);
            if let Some(Some(cont)) = attributes.get(b"continue".as_ref()) {
                if cont.value() == true {
                    r.extend(self.parse(xml)?);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(r)
    }
}
