use std::{collections::HashMap, io::BufReader};

use deno_runtime::{deno_core::v8, worker::MainWorker};
use maybe_xml::eval::bufread::BufReadEvaluator;

use crate::{anyhow::Result, xml_util, IncludeAdaptor};

use super::Script;

pub(crate) fn get_include_content<T: IncludeAdaptor>(
    script: &mut super::Script,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
) -> Result<Vec<u8>> {
    let src = crate::attr_parse_or_static_string(worker, attributes, b"src");
    let (xml, filename) = if let Some(xml) = include_adaptor.include(&src) {
        (Some(xml), src)
    } else {
        let substitute = crate::attr_parse_or_static_string(worker, attributes, b"substitute");
        if let Some(xml) = include_adaptor.include(&substitute) {
            (Some(xml), substitute)
        } else {
            (None, "".to_owned())
        }
    };
    if let Some(xml) = xml {
        if xml.len() > 0 {
            let var = crate::attr_parse_or_static_string(worker, attributes, b"var");
            let stack_push = if var.len() > 0 {
                worker.execute_script(
                    "stack.push",
                    ("wd.stack.push({".to_owned() + (&var).as_str() + "});").into(),
                )?;
                true
            } else {
                false
            };
            script.include_stack.push(filename);
            let r = script.parse(worker, xml.clone().as_slice(), b"", include_adaptor)?;
            script.include_stack.pop();
            if stack_push {
                worker.execute_script("stack.pop", "wd.stack.pop();".to_owned().into())?;
            }
            return Ok(r);
        }
    }

    Ok(b"".to_vec())
}

pub(super) fn case<T: IncludeAdaptor>(
    script: &mut Script,
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    xml: &[u8],
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    let mut r = Vec::new();

    let cmp_src = String::from_utf8(match attributes.get(b"value".as_slice()) {
        Some((None, Some(value))) => {
            let mut r = b"'".to_vec();
            r.append(&mut value.to_vec());
            r.append(&mut b"'".to_vec());
            r
        }
        Some((Some(prefix), Some(value))) => {
            if prefix == b"wd" {
                let mut r = b"(".to_vec();
                r.append(&mut value.to_vec());
                r.append(&mut b")".to_vec());
                r
            } else {
                b"''".to_vec()
            }
        }
        _ => b"''".to_vec(),
    })?;

    let mut tokenizer = BufReadEvaluator::from_reader(BufReader::new(xml)).into_iter();
    while let Some(token) = tokenizer.next() {
        match token {
            maybe_xml::token::owned::Token::StartTag(tag) => {
                let name = tag.name();
                if if let Some(prefix) = name.namespace_prefix() {
                    prefix.as_bytes() == b"wd"
                } else {
                    false
                } {
                    let attributes = crate::attr2map(&tag.attributes());
                    match name.local().as_bytes() {
                        b"when" => {
                            let xml = xml_util::inner(&name, &mut tokenizer);
                            if crate::eval_result(
                                &mut worker.js_runtime.handle_scope(),
                                &(cmp_src.to_owned()
                                    + "=="
                                    + String::from_utf8(
                                        if let Some((prefix, Some(value))) =
                                            attributes.get(b"value".as_slice())
                                        {
                                            if let Some(_) = prefix {
                                                value.to_vec()
                                            } else {
                                                let mut r = b"'".to_vec();
                                                r.append(&mut value.to_vec());
                                                r.append(&mut b"'".to_vec());
                                                r
                                            }
                                        } else {
                                            b"''".to_vec()
                                        },
                                    )?
                                    .as_str()),
                            ) == b"true"
                            {
                                r.append(&mut script.parse(
                                    worker,
                                    xml.as_slice(),
                                    b"",
                                    include_adaptor,
                                )?);
                                break;
                            }
                        }
                        b"else" => {
                            let xml = xml_util::inner(&name, &mut tokenizer);
                            r.append(&mut script.parse(
                                worker,
                                xml.as_slice(),
                                b"",
                                include_adaptor,
                            )?);
                        }
                        _ => {
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(r)
}

pub(super) fn r#if<T: IncludeAdaptor>(
    script: &mut Script,
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    xml: &[u8],
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    if crate::attr_parse_or_static(worker, attributes, b"value") == b"true" {
        return script.parse(worker, xml, b"", include_adaptor);
    }
    Ok(vec![])
}

pub(super) fn r#for<T: IncludeAdaptor>(
    script: &mut Script,
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    xml: &[u8],
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    let mut r = Vec::new();
    let var = crate::attr_parse_or_static_string(worker, attributes, b"var");
    if var != "" {
        if let Some((Some(prefix), Some(source))) = attributes.get(b"in".as_slice()) {
            if prefix == b"wd" {
                let (is_array, keys) = {
                    let mut is_array = true;
                    let mut keys = vec![];
                    let scope = &mut worker.js_runtime.handle_scope();
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
                        + (if let Some((None, Some(index))) = attributes.get(b"index".as_slice()) {
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
                        let scope = &mut worker.js_runtime.handle_scope();
                        let context = scope.get_current_context();
                        let scope = &mut v8::ContextScope::new(scope, context);
                        v8::String::new(scope, &source)
                            .and_then(|code| v8::Script::compile(scope, code, None))
                            .and_then(|v| v.run(scope));
                    }
                    r.append(&mut script.parse(worker, xml, b"", include_adaptor)?);
                    worker.execute_script("pop stack", "wd.stack.pop()".to_owned().into())?;
                }
            }
        }
    }
    Ok(r)
}
