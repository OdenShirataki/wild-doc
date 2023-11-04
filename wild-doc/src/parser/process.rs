use std::{borrow::Cow, ops::Deref, path::Path, sync::Arc};

use anyhow::Result;

use maybe_xml::{
    token::{prop::Attributes, Ty},
    Lexer,
};
use wild_doc_script::Vars;

use crate::xml_util;

use super::{Parser, WildDocValue};

impl Parser {
    pub(super) async fn get_include_content(&self, attr: Vars, vars: &Vars) -> Result<Vec<u8>> {
        if let Some(src) = attr.get("src") {
            let src = src.to_str();
            let (xml, filename) = self
                .include_adaptor
                .lock()
                .include(Path::new(src.as_ref()))
                .map_or_else(
                    || {
                        let mut r = (None, Cow::Borrowed(""));
                        if let Some(substitute) = attr.get("substitute") {
                            let substitute = substitute.to_str();
                            if let Some(xml) = self
                                .include_adaptor
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
                    self.include_stack.lock().push(filename.into());
                    let r = self.parse(xml.as_slice(), vars.clone()).await?;
                    self.include_stack.lock().pop();
                    return Ok(r);
                }
            }
        }
        Ok(b"".to_vec())
    }

    pub(super) async fn case(&self, attr: Vars, xml: &[u8], vars: &Vars) -> Result<Vec<u8>> {
        let cmp_src = attr.get("value");
        let mut pos = 0;
        let mut lexer = unsafe { Lexer::from_slice_unchecked(xml) };
        while let Some(token) = lexer.tokenize(&mut pos) {
            match token.ty() {
                Ty::StartTag(st) => {
                    let name = st.name();
                    match name.as_bytes() {
                        b"wd:when" => {
                            let begin = pos;
                            let (inner, _) = xml_util::to_end(&mut lexer, &mut pos);
                            if let Some(right) = self
                                .vars_from_attibutes(st.attributes(), vars)
                                .await
                                .get("value")
                            {
                                if let Some(cmp_src) = cmp_src {
                                    if cmp_src == right {
                                        return Ok(self
                                            .parse(&xml[begin..inner], vars.clone())
                                            .await?);
                                    }
                                }
                            }
                        }
                        b"wd:else" => {
                            let begin = pos;
                            let (inner, _) = xml_util::to_end(&mut lexer, &mut pos);
                            return Ok(self.parse(&xml[begin..inner], vars.clone()).await?);
                        }
                        _ => {}
                    }
                }
                Ty::EmptyElementTag(_)
                | Ty::EndTag(_)
                | Ty::Characters(_)
                | Ty::Cdata(_)
                | Ty::Comment(_)
                | Ty::Declaration(_) => {}
                _ => {
                    break;
                }
            }
        }
        Ok(vec![])
    }

    pub(super) async fn r#for(&self, attr: Vars, xml: &[u8], vars: &Vars) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        if let (Some(var), Some(r#in)) = (attr.get("var"), attr.get("in")) {
            let var = var.to_str();
            if var != "" {
                match r#in.deref() {
                    WildDocValue::Object(map) => {
                        if let Some(key_name) = attr.get("key") {
                            for (key, value) in map.into_iter() {
                                let mut new_vars = vars.clone();
                                new_vars.insert(var.to_string(), Arc::clone(value));
                                new_vars.insert(
                                    key_name.to_str().into(),
                                    Arc::new(serde_json::json!(key).into()),
                                );
                                r.extend(self.parse(xml, new_vars).await?);
                            }
                        } else {
                            for (_, value) in map.into_iter() {
                                let mut new_vars = vars.clone();
                                new_vars.insert(var.to_string(), Arc::clone(value));
                                r.extend(self.parse(xml, new_vars).await?);
                            }
                        }
                    }
                    WildDocValue::Array(vec) => {
                        let key_name = attr.get("key");
                        if let Some(key_name) = key_name {
                            let mut key = 0;
                            for value in vec.into_iter() {
                                key += 1;
                                let mut new_vars = vars.clone();
                                new_vars.insert(var.to_string(), Arc::clone(value));
                                new_vars.insert(
                                    key_name.to_str().into(),
                                    Arc::new(serde_json::json!(key).into()),
                                );
                                r.extend(self.parse(xml, new_vars).await?);
                            }
                        } else {
                            for value in vec.into_iter() {
                                let mut new_vars = vars.clone();
                                new_vars.insert(var.to_string(), Arc::clone(value));
                                r.extend(self.parse(xml, new_vars).await?);
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
        &self,
        attributes: Option<Attributes<'_>>,
        xml: &[u8],
        vars: &Vars,
    ) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        loop {
            if self
                .vars_from_attibutes(attributes, vars)
                .await
                .get("continue")
                .and_then(|v| v.as_bool())
                .map_or(false, |v| *v)
            {
                r.extend(self.parse(xml, vars.clone()).await?);
            } else {
                break;
            }
        }
        Ok(r)
    }
}
