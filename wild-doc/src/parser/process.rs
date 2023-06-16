use std::{collections::HashMap, rc::Rc};

use deno_runtime::deno_core::serde_json;
use maybe_xml::{
    scanner::{Scanner, State},
    token,
};

use crate::{anyhow::Result, xml_util, IncludeAdaptor};

use super::{AttributeMap, Parser, WildDocValue};

impl<T: IncludeAdaptor> Parser<T> {
    pub(super) fn get_include_content(&mut self, attributes: &AttributeMap) -> Result<Vec<u8>> {
        if let Some(Some(src)) = attributes.get(b"src".as_ref()) {
            let src = src.to_str();
            let (xml, filename) =
                if let Some(xml) = self.include_adaptor.lock().unwrap().include(src.as_ref()) {
                    (Some(xml), src.into_owned())
                } else {
                    let mut r = (None, "".to_owned());
                    if let Some(Some(substitute)) = attributes.get(b"substitute".as_ref()) {
                        let substitute = substitute.to_str();
                        if let Some(xml) = self
                            .include_adaptor
                            .lock()
                            .unwrap()
                            .include(substitute.as_ref())
                        {
                            r = (Some(xml), substitute.into_owned());
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

    pub(super) fn case(&mut self, attributes: &AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
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
                            let attributes = self.parse_attibutes(token.attributes());
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

    pub(super) fn r#if(&mut self, attributes: &AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        if let Some(Some(value)) = attributes.get(b"value".as_ref()) {
            if value.to_str() == "true" {
                return self.parse(xml);
            }
        }
        Ok(vec![])
    }

    pub(super) fn r#for(&mut self, attributes: &AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        if let (Some(Some(var)), Some(Some(r#in))) = (
            attributes.get(b"var".as_ref()),
            attributes.get(b"in".as_ref()),
        ) {
            let var = var.to_str();
            if var != "" {
                match &r#in.value {
                    serde_json::Value::Object(map) => {
                        for (key, value) in map {
                            let mut vars: HashMap<Vec<u8>, Rc<WildDocValue>> = HashMap::new();
                            vars.insert(
                                var.as_bytes().to_vec(),
                                Rc::new(WildDocValue::new(value.clone())),
                            );
                            if let Some(Some(key_name)) = attributes.get(b"key".as_ref()) {
                                vars.insert(
                                    key_name.to_str().as_bytes().to_vec(),
                                    Rc::new(WildDocValue::new(serde_json::json!(key))),
                                );
                            }
                            self.stack.write().unwrap().push(vars);
                            r.append(&mut self.parse(xml)?);
                            self.stack.write().unwrap().pop();
                        }
                    }
                    serde_json::Value::Array(vec) => {
                        let mut key = 0;
                        for value in vec {
                            let mut vars: HashMap<Vec<u8>, Rc<WildDocValue>> = HashMap::new();
                            vars.insert(
                                var.as_bytes().to_vec(),
                                Rc::new(WildDocValue::new(value.clone())),
                            );
                            if let Some(Some(key_name)) = attributes.get(b"key".as_ref()) {
                                vars.insert(
                                    key_name.to_str().as_bytes().to_vec(),
                                    Rc::new(WildDocValue::new(serde_json::json!(key))),
                                );
                            }
                            self.stack.write().unwrap().push(vars);
                            r.append(&mut self.parse(xml)?);
                            self.stack.write().unwrap().pop();
                            key += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(r)
    }
}
