use std::{borrow::Cow, path::Path, sync::Arc};

use anyhow::Result;

use maybe_xml::{
    scanner::{Scanner, State},
    token::{self, prop::Attributes},
};

use crate::xml_util;

use super::{AttributeMap, Parser, WildDocValue};

impl Parser {
    pub(super) async fn get_include_content(
        &mut self,
        attributes: AttributeMap,
    ) -> Result<Vec<u8>> {
        if let Some(Some(src)) = attributes.get(b"src".as_ref()) {
            let src = src.to_str();
            let (xml, filename) = self
                .state
                .include_adaptor()
                .lock()
                .include(Path::new(src.as_ref()))
                .map_or_else(
                    || {
                        let mut r = (None, Cow::Borrowed(""));
                        if let Some(Some(substitute)) = attributes.get(b"substitute".as_ref()) {
                            let substitute = substitute.to_str();
                            if let Some(xml) = self
                                .state
                                .include_adaptor()
                                .lock()
                                .include(Path::new(substitute.as_ref()))
                            {
                                r = (Some(xml), substitute);
                            }
                        }
                        r
                    },
                    |xml| (Some(xml), src),
                );
            if let Some(xml) = xml {
                if xml.len() > 0 {
                    self.include_stack.push(filename.to_string());
                    let r = self.parse(xml.as_slice()).await?;
                    self.include_stack.pop();
                    return Ok(r);
                }
            }
        }
        Ok(b"".to_vec())
    }

    pub(super) async fn case(&mut self, attributes: AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
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
                            if let Some(Some(right)) = self
                                .parse_attibutes(token.attributes())
                                .await
                                .get(b"value".as_ref())
                            {
                                if let Some(cmp_src) = cmp_src {
                                    if cmp_src == right {
                                        return Ok(self.parse(inner_xml).await?);
                                    }
                                }
                            }
                        }
                        b"wd:else" => {
                            return Ok(self.parse(xml_util::inner(xml).0).await?);
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

    pub(super) async fn r#if(&mut self, attributes: AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        if let Some(Some(value)) = attributes.get(b"value".as_ref()) {
            if value.as_bool().map_or(false, |v| *v) {
                return self.parse(xml).await;
            }
        }
        Ok(vec![])
    }

    pub(super) async fn r#for(&mut self, attributes: AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        if let (Some(Some(var)), Some(Some(r#in))) = (
            attributes.get(b"var".as_ref()),
            attributes.get(b"in".as_ref()),
        ) {
            let var = var.to_str();
            if var != "" {
                match r#in.as_ref() {
                    WildDocValue::Object(map) => {
                        if let Some(Some(key_name)) = attributes.get(b"key".as_ref()) {
                            for (key, value) in map {
                                self.state.stack().lock().push(
                                    [
                                        (var.to_string().into_bytes(), Arc::new(value.clone())),
                                        (
                                            key_name.to_string().into_bytes(),
                                            Arc::new(serde_json::json!(key).into()),
                                        ),
                                    ]
                                    .into(),
                                );
                                r.extend(self.parse(xml).await?);
                                self.state.stack().lock().pop();
                            }
                        } else {
                            for (_, value) in map {
                                self.state.stack().lock().push(
                                    [(var.to_string().into_bytes(), Arc::new(value.clone()))]
                                        .into(),
                                );
                                r.extend(self.parse(xml).await?);
                                self.state.stack().lock().pop();
                            }
                        }
                    }
                    WildDocValue::Array(vec) => {
                        let key_name = attributes.get(b"key".as_ref());
                        if let Some(Some(key_name)) = key_name {
                            let mut key = 0;
                            for value in vec {
                                key += 1;
                                self.state.stack().lock().push(
                                    [
                                        (var.to_string().into_bytes(), Arc::new(value.clone())),
                                        (
                                            key_name.to_string().into_bytes(),
                                            Arc::new(serde_json::json!(key).into()),
                                        ),
                                    ]
                                    .into(),
                                );
                                r.extend(self.parse(xml).await?);
                                self.state.stack().lock().pop();
                            }
                        } else {
                            for value in vec {
                                self.state.stack().lock().push(
                                    [(var.to_string().into_bytes(), Arc::new(value.clone()))]
                                        .into(),
                                );
                                r.extend(self.parse(xml).await?);
                                self.state.stack().lock().pop();
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(r)
    }

    pub(super) async fn r#while(
        &mut self,
        attributes: Option<Attributes<'_>>,
        xml: &[u8],
    ) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        loop {
            if self
                .parse_attibutes(attributes)
                .await
                .get(b"continue".as_ref())
                .and_then(|v| v.as_ref())
                .and_then(|v| v.as_bool())
                .map_or(false, |v| *v)
            {
                r.extend(self.parse(xml).await?);
            } else {
                break;
            }
        }
        Ok(r)
    }
}
