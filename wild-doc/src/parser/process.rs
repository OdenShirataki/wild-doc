use std::{borrow::Cow, ops::Deref, path::Path, sync::Arc};

use anyhow::Result;

use maybe_xml::{
    scanner::{Scanner, State},
    token::{self, prop::Attributes},
};
use wild_doc_script::{Vars, VarsStack};

use crate::xml_util;

use super::{Parser, WildDocValue};

impl Parser {
    pub(super) async fn get_include_content(
        &mut self,
        vars: Vars,
        stack: &mut VarsStack,
    ) -> Result<Vec<u8>> {
        if let Some(src) = vars.get("src") {
            let src = src.to_str();
            let (xml, filename) = self
                .state
                .include_adaptor()
                .lock()
                .include(Path::new(src.as_ref()))
                .map_or_else(
                    || {
                        let mut r = (None, Cow::Borrowed(""));
                        if let Some(substitute) = vars.get("substitute") {
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
                    self.include_stack.push(filename.into());
                    let r = self.parse(xml.as_slice(), stack).await?;
                    self.include_stack.pop();
                    return Ok(r);
                }
            }
        }
        Ok(b"".to_vec())
    }

    pub(super) async fn case(
        &mut self,
        vars: Vars,
        xml: &[u8],
        stack: &mut VarsStack,
    ) -> Result<Vec<u8>> {
        let cmp_src = vars.get("value");
        let mut xml = xml;
        let mut scanner = Scanner::new();
        while let Some(state) = scanner.scan(&xml) {
            match state {
                State::ScannedStartTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::StartTag::from(token_bytes);
                    let name = token.name();

                    match name.as_bytes() {
                        b"wd:when" => {
                            let (inner_xml, outer_end) = xml_util::inner(xml);
                            xml = &xml[outer_end..];
                            if let Some(right) = self
                                .vars_from_attibutes(token.attributes(), stack)
                                .await
                                .get("value")
                            {
                                if let Some(cmp_src) = cmp_src {
                                    if cmp_src == right {
                                        return Ok(self.parse(inner_xml, stack).await?);
                                    }
                                }
                            }
                        }
                        b"wd:else" => {
                            return Ok(self.parse(xml_util::inner(xml).0, stack).await?);
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

    pub(super) async fn r#if(
        &mut self,
        vars: Vars,
        xml: &[u8],
        stack: &mut VarsStack,
    ) -> Result<Vec<u8>> {
        if let Some(value) = vars.get("value") {
            if value.as_bool().map_or(false, |v| *v) {
                return self.parse(xml, stack).await;
            }
        }
        Ok(vec![])
    }

    pub(super) async fn r#for(
        &mut self,
        vars: Vars,
        xml: &[u8],
        stack: &mut VarsStack,
    ) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        if let (Some(var), Some(r#in)) = (vars.get("var"), vars.get("in")) {
            let var = var.to_str();
            if var != "" {
                match r#in.deref() {
                    WildDocValue::Object(map) => {
                        if let Some(key_name) = vars.get("key") {
                            for (key, value) in map.into_iter() {
                                stack.push(
                                    [
                                        (var.to_string(), Arc::clone(value)),
                                        (
                                            key_name.to_str().into(),
                                            Arc::new(serde_json::json!(key).into()),
                                        ),
                                    ]
                                    .into(),
                                );
                                r.extend(self.parse(xml, stack).await?);
                                stack.pop();
                            }
                        } else {
                            for (_, value) in map.into_iter() {
                                stack.push([(var.to_string(), Arc::clone(value))].into());
                                r.extend(self.parse(xml, stack).await?);
                                stack.pop();
                            }
                        }
                    }
                    WildDocValue::Array(vec) => {
                        let key_name = vars.get("key");
                        if let Some(key_name) = key_name {
                            let mut key = 0;
                            for value in vec.into_iter() {
                                key += 1;
                                stack.push(
                                    [
                                        (var.to_string(), Arc::clone(value)),
                                        (
                                            key_name.to_str().into(),
                                            Arc::new(serde_json::json!(key).into()),
                                        ),
                                    ]
                                    .into(),
                                );
                                r.extend(self.parse(xml, stack).await?);
                                stack.pop();
                            }
                        } else {
                            for value in vec.into_iter() {
                                stack.push([(var.to_string(), Arc::clone(value))].into());
                                r.extend(self.parse(xml, stack).await?);
                                stack.pop();
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
        stack: &mut VarsStack,
    ) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        loop {
            if self
                .vars_from_attibutes(attributes, stack)
                .await
                .get("continue")
                .and_then(|v| v.as_bool())
                .map_or(false, |v| *v)
            {
                r.extend(self.parse(xml, stack).await?);
            } else {
                break;
            }
        }
        Ok(r)
    }
}
