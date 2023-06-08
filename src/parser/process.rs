use deno_runtime::deno_core::v8;
use maybe_xml::{
    scanner::{Scanner, State},
    token,
};

use crate::{anyhow::Result, xml_util, IncludeAdaptor};

use super::{AttributeMap, Parser};

impl<T: IncludeAdaptor> Parser<T> {
    pub(super) fn get_include_content(&mut self, attributes: &AttributeMap) -> Result<Vec<u8>> {
        if let Some(Some(src)) = attributes.get(b"src".as_ref()) {
            let (xml, filename) = if let Some(xml) =
                self.include_adaptor.lock().unwrap().include(&src)
            {
                (Some(xml), src.to_owned())
            } else {
                let mut r = (None, "".to_owned());
                if let Some(Some(substitute)) = attributes.get(b"substitute".as_ref()) {
                    if let Some(xml) = self.include_adaptor.lock().unwrap().include(&substitute) {
                        r = (Some(xml), substitute.to_owned());
                    }
                }
                r
            };
            if let Some(xml) = xml {
                if xml.len() > 0 {
                    let mut stack_push = false;
                    if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
                        if var.len() > 0 {
                            self.deno.execute_script(
                                "stack.push",
                                ("wd.stack.push({".to_owned() + var.as_str() + "});").into(),
                            )?;
                            stack_push = true;
                        }
                    }
                    self.include_stack.push(filename);
                    let r = self.parse(xml.as_slice())?;
                    self.include_stack.pop();
                    if stack_push {
                        self.deno
                            .execute_script("stack.pop", "wd.stack.pop();".to_owned().into())?;
                    }
                    return Ok(r);
                }
            }
        }
        Ok(b"".to_vec())
    }

    pub(super) fn case(&mut self, attributes: &AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        let cmp_src = if let Some(Some(value)) = attributes.get(b"value".as_ref()) {
            value
        } else {
            ""
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
                                if cmp_src == right {
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
            if value == "true" {
                return self.parse(xml);
            }
        }
        Ok(vec![])
    }

    pub(super) fn r#for(&mut self, attributes: &AttributeMap, xml: &[u8]) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        if let (Some(Some(var)), Some(Some(source))) = (
            attributes.get(b"var".as_ref()),
            attributes.get(b"in".as_ref()),
        ) {
            if var != "" {
                let (is_array, keys) = {
                    let mut is_array = true;
                    let mut keys = vec![];
                    let scope = &mut self.deno.js_runtime.handle_scope();
                    let context = scope.get_current_context();
                    let scope = &mut v8::ContextScope::new(scope, context);
                    if let Some(rs) = {
                        v8::String::new(scope, &source)
                            .and_then(|code| v8::Script::compile(scope, code, None))
                            .and_then(|code| code.run(scope))
                    } {
                        if rs.is_array() {
                            let rs = unsafe { v8::Local::<v8::Array>::cast(rs) };
                            let length = rs.length();
                            for i in 0..length {
                                keys.push(i.to_string());
                            }
                        } else if rs.is_object() {
                            is_array = false;
                            let rs = unsafe { v8::Local::<v8::Object>::cast(rs) };
                            if let Some(names) = rs.get_property_names(scope, Default::default()) {
                                for i in 0..names.length() {
                                    if let Some(name) = names.get_index(scope, i) {
                                        keys.push(name.to_rust_string_lossy(scope));
                                    }
                                }
                            }
                        }
                    }
                    (is_array, keys)
                };
                let index = attributes.get(b"index".as_ref());
                for i in keys {
                    let key_str = if is_array {
                        i.to_owned()
                    } else {
                        "'".to_owned() + i.as_str() + "'"
                    };

                    let source = "wd.stack.push({".to_owned()
                        + var.as_str()
                        + ":("
                        + source.as_str()
                        + ")["
                        + key_str.as_str()
                        + "]"
                        + (if let Some(Some(ref index)) = index {
                            if index.len() > 0 {
                                ",".to_owned() + index.as_str() + ":" + key_str.as_str()
                            } else {
                                "".to_owned()
                            }
                        } else {
                            "".to_owned()
                        })
                        .as_str()
                        + "})";
                    {
                        let scope = &mut self.deno.js_runtime.handle_scope();
                        let context = scope.get_current_context();
                        let scope = &mut v8::ContextScope::new(scope, context);
                        v8::String::new(scope, &source)
                            .and_then(|code| v8::Script::compile(scope, code, None))
                            .and_then(|v| v.run(scope));
                    }
                    r.append(&mut self.parse(xml)?);
                    self.deno
                        .execute_script("pop stack", "wd.stack.pop()".to_owned().into())?;
                }
            }
        }
        Ok(r)
    }
}
