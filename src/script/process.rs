use std::collections::HashMap;

use deno_runtime::deno_core::v8;
use maybe_xml::{
    scanner::{Scanner, State},
    token,
};

use crate::{anyhow::Result, xml_util, IncludeAdaptor};

use super::Script;

impl<T: IncludeAdaptor> Script<T> {
    pub(super) fn get_include_content(
        &mut self,
        attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    ) -> Result<Vec<u8>> {
        let src = crate::attr_parse_or_static_string(&mut self.worker, attributes, b"src");
        let (xml, filename) = if let Some(xml) = self.include_adaptor.lock().unwrap().include(&src)
        {
            (Some(xml), src)
        } else {
            let substitute =
                crate::attr_parse_or_static_string(&mut self.worker, attributes, b"substitute");
            if let Some(xml) = self.include_adaptor.lock().unwrap().include(&substitute) {
                (Some(xml), substitute)
            } else {
                (None, "".to_owned())
            }
        };
        if let Some(xml) = xml {
            if xml.len() > 0 {
                let var = crate::attr_parse_or_static_string(&mut self.worker, attributes, b"var");
                let stack_push = if var.len() > 0 {
                    self.worker.execute_script(
                        "stack.push",
                        ("wd.stack.push({".to_owned() + (&var).as_str() + "});").into(),
                    )?;
                    true
                } else {
                    false
                };
                self.include_stack.push(filename);
                let r = self.parse(xml.as_slice())?;
                self.include_stack.pop();
                if stack_push {
                    self.worker
                        .execute_script("stack.pop", "wd.stack.pop();".to_owned().into())?;
                }
                return Ok(r);
            }
        }

        Ok(b"".to_vec())
    }

    pub(super) fn case(
        &mut self,
        attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
        xml: &[u8],
    ) -> Result<Vec<u8>> {
        let cmp_src = match attributes.get(b"value".as_slice()) {
            Some((None, Some(value))) => {
                let mut r = value.to_vec();
                r.push(b'\'');
                r
            }
            Some((Some(prefix), Some(value))) => {
                if prefix == b"wd" {
                    let mut r = vec![b'('];
                    r.append(&mut value.to_vec());
                    r.push(b')');
                    r
                } else {
                    b"''".to_vec()
                }
            }
            _ => b"''".to_vec(),
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
                            let cmp = String::from_utf8(cmp_src.clone())?
                                + "=="
                                + std::str::from_utf8(&if let Some((prefix, Some(value))) =
                                    crate::attr2map(&token.attributes()).get(b"value".as_slice())
                                {
                                    if let Some(_) = prefix {
                                        value.to_vec()
                                    } else {
                                        let mut r = vec![b'\''];
                                        r.append(&mut value.to_vec());
                                        r.push(b'\'');
                                        r
                                    }
                                } else {
                                    vec![b'\'', b'\'']
                                })?;
                            if crate::eval_result(
                                &mut self.worker.js_runtime.handle_scope(),
                                cmp.as_str(),
                            ) == b"true"
                            {
                                return Ok(self.parse(inner_xml)?);
                            }
                            xml = &xml[outer_end..];
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

    pub(super) fn r#if(
        &mut self,
        attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
        xml: &[u8],
    ) -> Result<Vec<u8>> {
        if crate::attr_parse_or_static(&mut self.worker, attributes, b"value") == b"true" {
            return self.parse(xml);
        }
        Ok(vec![])
    }

    pub(super) fn r#for(
        &mut self,
        attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
        xml: &[u8],
    ) -> Result<Vec<u8>> {
        let mut r = Vec::new();
        let var = crate::attr_parse_or_static_string(&mut self.worker, attributes, b"var");
        if var != "" {
            if let Some((Some(prefix), Some(source))) = attributes.get(b"in".as_slice()) {
                if prefix == b"wd" {
                    let (is_array, keys) = {
                        let mut is_array = true;
                        let mut keys = vec![];
                        let scope = &mut self.worker.js_runtime.handle_scope();
                        let context = scope.get_current_context();
                        let scope = &mut v8::ContextScope::new(scope, context);
                        if let Some(rs) = {
                            v8::String::new(scope, std::str::from_utf8(&source)?)
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
                                if let Some(names) =
                                    rs.get_property_names(scope, Default::default())
                                {
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
                    let source = std::str::from_utf8(source)?;
                    for i in keys {
                        let key_str = if is_array {
                            i.to_owned()
                        } else {
                            "'".to_owned() + i.as_str() + "'"
                        };

                        let source = "wd.stack.push({".to_owned()
                            + var.as_str()
                            + ":("
                            + source
                            + ")["
                            + key_str.as_str()
                            + "]"
                            + (if let Some((None, Some(index))) =
                                attributes.get(b"index".as_slice())
                            {
                                if index.len() > 0 {
                                    ",".to_owned()
                                        + std::str::from_utf8(index)?
                                        + ":"
                                        + key_str.as_str()
                                } else {
                                    "".to_owned()
                                }
                            } else {
                                "".to_owned()
                            })
                            .as_str()
                            + "})";
                        {
                            let scope = &mut self.worker.js_runtime.handle_scope();
                            let context = scope.get_current_context();
                            let scope = &mut v8::ContextScope::new(scope, context);
                            v8::String::new(scope, &source)
                                .and_then(|code| v8::Script::compile(scope, code, None))
                                .and_then(|v| v.run(scope));
                        }
                        r.append(&mut self.parse(xml)?);
                        self.worker
                            .execute_script("pop stack", "wd.stack.pop()".to_owned().into())?;
                    }
                }
            }
        }
        Ok(r)
    }
}
