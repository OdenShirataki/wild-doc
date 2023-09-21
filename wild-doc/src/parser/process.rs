use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use anyhow::Result;

use bson::Bson;
use maybe_xml::{
    scanner::{Scanner, State},
    token::{self, prop::Attributes},
};

use crate::xml_util;

use super::{AttributeMap, Parser};

impl Parser {
    #[inline(always)]
    pub(super) fn get_include_content(&mut self, attributes: AttributeMap) -> Result<Vec<u8>> {
        if let Some(Some(src)) = attributes.get(b"src".as_ref()) {
            if let Some(src) = src.as_str() {
                let (xml, filename) = self
                    .state
                    .include_adaptor()
                    .lock()
                    .unwrap()
                    .include(src.into())
                    .map_or_else(
                        || {
                            let mut r = (None, "".to_owned());
                            if let Some(Some(substitute)) = attributes.get(b"substitute".as_ref()) {
                                if let Some(substitute) = substitute.as_str() {
                                    if let Some(xml) = self
                                        .state
                                        .include_adaptor()
                                        .lock()
                                        .unwrap()
                                        .include(substitute.into())
                                    {
                                        r = (Some(xml), substitute.to_owned());
                                    }
                                }
                            }
                            r
                        },
                        |xml| (Some(xml), src.to_owned().into()),
                    );
                if let Some(xml) = xml {
                    if xml.len() > 0 {
                        self.include_stack.push(filename);
                        let r = self.parse(xml.as_slice())?;
                        self.include_stack.pop();
                        return Ok(r);
                    }
                }
            }
        }
        Ok(b"".to_vec())
    }

    #[inline(always)]
    pub(super) fn case(&mut self, attributes: AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        let cmp_src = attributes
            .get(b"value".as_ref())
            .and_then(|v| v.as_ref())
            .map(|v| v);
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
                                if let Some(cmp_src) = cmp_src {
                                    if cmp_src.to_str() == right.to_str() {
                                        return Ok(self.parse(inner_xml)?);
                                    }
                                }
                            }
                        }
                        b"wd:else" => {
                            return Ok(self.parse(xml_util::inner(xml).0)?);
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

    #[inline(always)]
    pub(super) fn r#if(&mut self, attributes: AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        if let Some(Some(value)) = attributes.get(b"value".as_ref()) {
            if value.to_str() == "true" {
                return self.parse(xml);
            }
        }
        Ok(vec![])
    }

    #[inline(always)]
    pub(super) fn r#for(&mut self, attributes: AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        if let (Some(Some(var)), Some(Some(r#in))) = (
            attributes.get(b"var".as_ref()),
            attributes.get(b"in".as_ref()),
        ) {
            let var = var.to_str();
            if var != "" {
                match r#in.as_ref() {
                    WildDocValue::Object(map) => {
                        let key_name = attributes.get(b"key".as_ref());
                        for (key, value) in map {
                            let mut vars = HashMap::new();
                            vars.insert(
                                var.to_string().into_bytes(),
                                Arc::new(RwLock::new(WildDocValue::from(value.clone()))),
                            );
                            if let Some(Some(key_name)) = key_name {
                                vars.insert(
                                    key_name.to_string().into_bytes(),
                                    Arc::new(RwLock::new(WildDocValue::from(serde_json::json!(
                                        key
                                    )))),
                                );
                            }
                            self.state.stack().write().unwrap().push(vars);
                            r.extend(self.parse(xml)?);
                            self.state.stack().write().unwrap().pop();
                        }
                    }
                    WildDocValue::Array(vec) => {
                        let key_name = attributes.get(b"key".as_ref());
                        let mut key = 0;
                        for value in vec {
                            let mut vars = HashMap::new();
                            vars.insert(
                                var.to_string().into_bytes(),
                                Arc::new(RwLock::new(value.clone())),
                            );
                            if let Some(Some(key_name)) = key_name {
                                vars.insert(
                                    key_name.to_string().into_bytes(),
                                    Arc::new(RwLock::new(WildDocValue::Number(
                                        serde_json::Number::from(key),
                                    ))),
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

    #[inline(always)]
    pub(super) fn r#while(
        &mut self,
        attributes: Option<Attributes<'_>>,
        xml: &[u8],
    ) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        loop {
            let attributes = self.parse_attibutes(&attributes);
            if let Some(Some(cont)) = attributes.get(b"continue".as_ref()) {
                if cont.to_str() == "true" {
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
