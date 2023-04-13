use deno_runtime::{
    deno_core::{error::AnyError, v8},
    worker::MainWorker,
};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};

use crate::{
    xml_util::{self, XmlAttr},
    IncludeAdaptor,
};

use super::Script;

pub fn get_include_content<T: IncludeAdaptor>(
    script: &mut super::Script,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
    attr: &XmlAttr,
) -> Result<Vec<u8>, AnyError> {
    let src = crate::attr_parse_or_static_string(worker, attr, "src");
    let (xml, filename) = if let Some(xml) = include_adaptor.include(&src) {
        (Some(xml), src)
    } else {
        let substitute = crate::attr_parse_or_static_string(worker, attr, "substitute");
        if let Some(xml) = include_adaptor.include(&substitute) {
            (Some(xml), substitute)
        } else {
            (None, "".to_owned())
        }
    };
    if let Some(xml) = xml {
        if xml.len() > 0 {
            if let Ok(xml) = std::str::from_utf8(xml) {
                let mut r = vec![];

                let var = crate::attr_parse_or_static_string(worker, attr, "var");
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
                r.append(&mut run_xml(
                    script,
                    "<r>".to_owned() + xml + "</r>",
                    worker,
                    include_adaptor,
                )?);
                script.include_stack.pop();
                if stack_push {
                    worker.execute_script("stack.pop", "wd.stack.pop();".to_owned().into())?;
                }
                return Ok(r);
            }
        }
    }

    Ok(b"".to_vec())
}

fn run_xml<T: IncludeAdaptor>(
    script: &mut Script,
    xml: String,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>, AnyError> {
    let mut event_reader_inner = quick_xml::Reader::from_str(&xml);
    event_reader_inner.check_end_names(false);

    if let Ok(quick_xml::events::Event::Start(e)) = event_reader_inner.read_event() {
        return script.parse(
            worker,
            &mut event_reader_inner,
            e.name().as_ref(),
            include_adaptor,
        );
    }
    Ok(b"".to_vec())
}

pub(super) fn case<T: IncludeAdaptor>(
    script: &mut Script,
    e: &BytesStart,
    xml_str: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>, AnyError> {
    let mut r = Vec::new();
    let attr = xml_util::attr2hash_map(&e);

    if let Ok(cmp_src) = String::from_utf8(if let Some(value) = attr.get("wd:value") {
        let mut r = b"(".to_vec();
        r.append(&mut value.to_vec());
        r.append(&mut b")".to_vec());
        r
    } else if let Some(value) = attr.get("value") {
        let mut r = b"'".to_vec();
        r.append(&mut value.to_vec());
        r.append(&mut b"'".to_vec());
        r
    } else {
        b"''".to_vec()
    }) {
        let mut event_reader = Reader::from_str(&xml_str.trim());
        event_reader.check_end_names(false);
        loop {
            match event_reader.read_event() {
                Ok(Event::Start(e)) => {
                    if e.name().as_ref() == b"wd:case" {
                        'case: loop {
                            if let Ok(next) = event_reader.read_event() {
                                match next {
                                    Event::Start(ref e) => {
                                        let name = e.name();
                                        match name.as_ref() {
                                            b"wd:when" => {
                                                let xml_str =
                                                    xml_util::outer(&next, name, &mut event_reader);

                                                let attr = xml_util::attr2hash_map(&e);
                                                if let Ok(src) = String::from_utf8(
                                                    if let Some(value) = attr.get("wd:value") {
                                                        value.to_vec()
                                                    } else if let Some(value) = attr.get("value") {
                                                        let mut r = b"'".to_vec();
                                                        r.append(&mut value.to_vec());
                                                        r.append(&mut b"'".to_vec());
                                                        r
                                                    } else {
                                                        b"''".to_vec()
                                                    },
                                                ) {
                                                    if crate::eval_result(
                                                        &mut worker.js_runtime.handle_scope(),
                                                        &(cmp_src.to_owned() + "==" + src.as_str()),
                                                    ) == b"true"
                                                    {
                                                        let mut event_reader_inner =
                                                            Reader::from_str(&xml_str.trim());
                                                        event_reader_inner.check_end_names(false);
                                                        loop {
                                                            match event_reader_inner.read_event() {
                                                                Ok(Event::Start(e)) => {
                                                                    if e.name().as_ref()
                                                                        == b"wd:when"
                                                                    {
                                                                        r.append(&mut script.parse(
                                                                            worker,
                                                                            &mut event_reader_inner,
                                                                            b"",
                                                                            include_adaptor,
                                                                        )?);
                                                                        break 'case;
                                                                    }
                                                                }
                                                                _ => {}
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            b"wd:else" => {
                                                let xml_str =
                                                    xml_util::outer(&next, name, &mut event_reader);
                                                let mut event_reader_inner =
                                                    Reader::from_str(&xml_str.trim());
                                                event_reader_inner.check_end_names(false);
                                                loop {
                                                    match event_reader_inner.read_event() {
                                                        Ok(Event::Start(e)) => {
                                                            if e.name().as_ref() == b"wd:else" {
                                                                r.append(&mut script.parse(
                                                                    worker,
                                                                    &mut event_reader_inner,
                                                                    b"",
                                                                    include_adaptor,
                                                                )?);
                                                                break;
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                            _ => {
                                                break;
                                            }
                                        }
                                    }
                                    Event::Eof => {
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(r)
}

pub(super) fn re<T: IncludeAdaptor>(
    script: &mut Script,
    xml_str: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>, AnyError> {
    let mut r = Vec::new();

    let mut event_reader = Reader::from_str(&xml_str.trim());
    event_reader.check_end_names(false);
    match event_reader.read_event() {
        Ok(Event::Start(e)) => {
            if e.name().as_ref() == b"wd:re" {
                if let Ok(parsed) = script.parse(worker, &mut event_reader, b"", include_adaptor) {
                    if let Ok(parsed) = std::str::from_utf8(&parsed) {
                        r.append(&mut run_xml(
                            script,
                            "<r>".to_owned() + parsed + "</r>",
                            worker,
                            include_adaptor,
                        )?);
                    }
                }
            }
        }
        _ => {}
    }

    Ok(r)
}

pub(super) fn r#if<T: IncludeAdaptor>(
    script: &mut Script,
    e: &BytesStart,
    xml_str: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>, AnyError> {
    let mut r = Vec::new();
    let attr = xml_util::attr2hash_map(&e);

    if crate::attr_parse_or_static(worker, &attr, "value") == b"true" {
        let mut event_reader = Reader::from_str(&xml_str.trim());
        event_reader.check_end_names(false);
        match event_reader.read_event() {
            Ok(Event::Start(e)) => {
                if e.name().as_ref() == b"wd:if" {
                    r.append(&mut script.parse(worker, &mut event_reader, b"", include_adaptor)?);
                }
            }
            _ => {}
        }
    }

    Ok(r)
}
pub(super) fn r#for<T: IncludeAdaptor>(
    script: &mut Script,
    e: &BytesStart,
    xml_str: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>, AnyError> {
    let mut r = Vec::new();
    let attr = xml_util::attr2hash_map(&e);
    let var = crate::attr_parse_or_static_string(worker, &attr, "var");
    if var != "" {
        if let Some(source) = attr.get("wd:in") {
            if let Ok(source) = std::str::from_utf8(source) {
                let (is_array, keys) = {
                    let mut is_array = true;
                    let mut keys = vec![];
                    let scope = &mut worker.js_runtime.handle_scope();
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
                for i in keys {
                    let mut ev = Reader::from_str(&xml_str);
                    ev.check_end_names(false);
                    loop {
                        match ev.read_event() {
                            Ok(Event::Start(e)) => {
                                if e.name().as_ref() == b"wd:for" {
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
                                        + (if let Ok(Some(index)) = e.try_get_attribute(b"index") {
                                            std::str::from_utf8(&index.value)
                                                .map_or("".to_string(), |v| {
                                                    ",".to_owned() + v + ":" + key_str.as_str()
                                                })
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
                                    r.append(&mut script.parse(
                                        worker,
                                        &mut ev,
                                        b"wd:for",
                                        include_adaptor,
                                    )?);
                                    worker.execute_script(
                                        "pop stack",
                                        "wd.stack.pop()".to_owned().into(),
                                    )?;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Ok(r)
}
