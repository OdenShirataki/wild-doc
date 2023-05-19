use std::collections::HashMap;

use deno_runtime::{deno_core::v8, worker::MainWorker};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use xmlparser::{Token, Tokenizer};

use crate::{
    anyhow::Result,
    xml_util::{self, XmlAttr},
    IncludeAdaptor,
};

use super::Script;

pub(crate) fn get_include_content_xml_parser<T: IncludeAdaptor>(
    script: &mut super::Script,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
    attributes: &HashMap<(String, String), String>,
) -> Result<Vec<u8>> {
    let src = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "src");
    println!("include src : {}", src.as_str());
    let (xml, filename) = if let Some(xml) = include_adaptor.include(&src) {
        (Some(xml), src)
    } else {
        let substitute =
            crate::attr_parse_or_static_string_xml_parser(worker, attributes, "substitute");
        if let Some(xml) = include_adaptor.include(&substitute) {
            (Some(xml), substitute)
        } else {
            (None, "".to_owned())
        }
    };
    if let Some(xml) = xml {
        if xml.len() > 0 {
            let var = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "var");
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
            let r = run_xml_xml_parser(
                script,
                "<r>".to_owned() + std::str::from_utf8(xml)? + "</r>",
                worker,
                include_adaptor,
            )?;
            script.include_stack.pop();
            if stack_push {
                worker.execute_script("stack.pop", "wd.stack.pop();".to_owned().into())?;
            }
            return Ok(r);
        }
    }

    Ok(b"".to_vec())
}

pub(crate) fn get_include_content<T: IncludeAdaptor>(
    script: &mut super::Script,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
    attr: &XmlAttr,
) -> Result<Vec<u8>> {
    let src = crate::attr_parse_or_static_string(worker, attr, "src");
    println!("include src : {}", src.as_str());
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
            let r = run_xml(
                script,
                "<r>".to_owned() + std::str::from_utf8(xml)? + "</r>",
                worker,
                include_adaptor,
            )?;
            script.include_stack.pop();
            if stack_push {
                worker.execute_script("stack.pop", "wd.stack.pop();".to_owned().into())?;
            }
            return Ok(r);
        }
    }

    Ok(b"".to_vec())
}

fn run_xml_xml_parser<T: IncludeAdaptor>(
    script: &mut Script,
    xml: String,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    let mut tokenizer = xmlparser::Tokenizer::from(xml.as_str());
    if let Some(Ok(Token::ElementStart { prefix, local, .. })) = tokenizer.next() {
        tokenizer.next(); // maybe />
        script.parse_xml_parser(
            worker,
            &mut tokenizer,
            (prefix.as_str(), local.as_str()),
            include_adaptor,
        )
    } else {
        Ok(b"".to_vec())
    }
}
fn run_xml<T: IncludeAdaptor>(
    script: &mut Script,
    xml: String,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    let mut event_reader_inner = Reader::from_str(&xml);
    event_reader_inner.check_end_names(false);
    if let Event::Start(e) = event_reader_inner.read_event()? {
        script.parse(
            worker,
            &mut event_reader_inner,
            e.name().as_ref(),
            include_adaptor,
        )
    } else {
        Ok(b"".to_vec())
    }
}

pub(super) fn case<T: IncludeAdaptor>(
    script: &mut Script,
    e: &BytesStart,
    xml_str: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    let mut r = Vec::new();
    let attr = xml_util::attr2hash_map(&e);

    let cmp_src = String::from_utf8(if let Some(value) = attr.get("wd:value") {
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
    })?;

    let mut event_reader = Reader::from_str(&xml_str);
    event_reader.check_end_names(false);
    loop {
        match event_reader.read_event()? {
            Event::Start(e) => {
                if e.name().as_ref() == b"wd:case" {
                    'case: loop {
                        let next = event_reader.read_event()?;
                        match next {
                            Event::Start(ref e) => {
                                let name = e.name();
                                match name.as_ref() {
                                    b"wd:when" => {
                                        let xml_str =
                                            xml_util::outer(&next, name, &mut event_reader);
                                        let attr = xml_util::attr2hash_map(&e);
                                        if crate::eval_result(
                                            &mut worker.js_runtime.handle_scope(),
                                            &(cmp_src.to_owned()
                                                + "=="
                                                + String::from_utf8(
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
                                                )?
                                                .as_str()),
                                        ) == b"true"
                                        {
                                            let mut event_reader_inner = Reader::from_str(&xml_str);
                                            event_reader_inner.check_end_names(false);
                                            loop {
                                                match event_reader_inner.read_event()? {
                                                    Event::Start(e) => {
                                                        if e.name().as_ref() == b"wd:when" {
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
                                    b"wd:else" => {
                                        let xml_str =
                                            xml_util::outer(&next, name, &mut event_reader);
                                        let mut event_reader_inner = Reader::from_str(&xml_str);
                                        event_reader_inner.check_end_names(false);
                                        loop {
                                            match event_reader_inner.read_event()? {
                                                Event::Start(e) => {
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
                    break;
                }
            }
            _ => {}
        }
    }

    Ok(r)
}

pub(super) fn case_xml_parser<T: IncludeAdaptor>(
    script: &mut Script,
    attributes: &HashMap<(String, String), String>,
    xml: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    let mut r = Vec::new();

    let cmp_src = String::from_utf8(
        if let Some(value) = attributes.get(&("wd".to_owned(), "value".to_owned())) {
            let mut r = b"(".to_vec();
            r.append(&mut value.clone().into_bytes());
            r.append(&mut b")".to_vec());
            r
        } else if let Some(value) = attributes.get(&("".to_owned(), "value".to_owned())) {
            let mut r = b"'".to_vec();
            r.append(&mut value.clone().into_bytes());
            r.append(&mut b"'".to_vec());
            r
        } else {
            b"''".to_vec()
        },
    )?;

    let mut tokenizer = Tokenizer::from(xml);
    while let Some(Ok(token)) = tokenizer.next() {
        match token {
            Token::ElementStart { prefix, local, .. } => {
                if prefix.as_str() == "wd" && local.as_str() == "case" {
                    'case: loop {
                        if let Some(Ok(token)) = tokenizer.next() {
                            match token {
                                Token::ElementStart {
                                    prefix,
                                    local,
                                    span,
                                } => {
                                    let (attributes_str, attributes) =
                                        xml_util::attributes(&mut tokenizer);
                                    let prefix = prefix.as_str();
                                    if prefix == "wd" {
                                        let local = local.as_str();
                                        match local {
                                            "when" => {
                                                let xml = xml_util::outer_xml_parser(
                                                    span.as_str(),
                                                    &attributes_str,
                                                    prefix,
                                                    local,
                                                    &mut tokenizer,
                                                );
                                                if crate::eval_result(
                                                    &mut worker.js_runtime.handle_scope(),
                                                    &(cmp_src.to_owned()
                                                        + "=="
                                                        + String::from_utf8(
                                                            if let Some(value) = attributes.get(&(
                                                                "wd".to_owned(),
                                                                "value".to_owned(),
                                                            )) {
                                                                value.clone().into_bytes()
                                                            } else if let Some(value) = attributes
                                                                .get(&(
                                                                    "".to_owned(),
                                                                    "value".to_owned(),
                                                                ))
                                                            {
                                                                let mut r = b"'".to_vec();
                                                                r.append(
                                                                    &mut value.clone().into_bytes(),
                                                                );
                                                                r.append(&mut b"'".to_vec());
                                                                r
                                                            } else {
                                                                b"''".to_vec()
                                                            },
                                                        )?
                                                        .as_str()),
                                                ) == b"true"
                                                {
                                                    let mut tokenizer =
                                                        xmlparser::Tokenizer::from(xml.as_str());
                                                    while let Some(Ok(token)) = tokenizer.next() {
                                                        match token {
                                                            Token::ElementStart {
                                                                prefix,
                                                                local,
                                                                ..
                                                            } => {
                                                                let prefix = prefix.as_str();
                                                                let local = local.as_str();
                                                                if prefix == "wd" && local == "when"
                                                                {
                                                                    r.append(
                                                                        &mut script
                                                                            .parse_xml_parser(
                                                                                worker,
                                                                                &mut tokenizer,
                                                                                ("", ""),
                                                                                include_adaptor,
                                                                            )?,
                                                                    );
                                                                    break 'case;
                                                                }
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                            }
                                            "else" => {
                                                let xml = xml_util::outer_xml_parser(
                                                    span.as_str(),
                                                    &attributes_str,
                                                    prefix,
                                                    local,
                                                    &mut tokenizer,
                                                );
                                                let mut tokenizer =
                                                    xmlparser::Tokenizer::from(xml.as_str());
                                                while let Some(Ok(token)) = tokenizer.next() {
                                                    match token {
                                                        Token::ElementStart {
                                                            prefix,
                                                            local,
                                                            ..
                                                        } => {
                                                            let prefix = prefix.as_str();
                                                            let local = local.as_str();
                                                            if prefix == "wd" && local == "else" {
                                                                r.append(
                                                                    &mut script.parse_xml_parser(
                                                                        worker,
                                                                        &mut tokenizer,
                                                                        ("", ""),
                                                                        include_adaptor,
                                                                    )?,
                                                                );
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
                                }
                                _ => {}
                            }
                        } else {
                            break;
                        }
                    }
                    break;
                }
            }
            _ => {}
        }
    }
    Ok(r)
}

pub(super) fn re<T: IncludeAdaptor>(
    script: &mut Script,
    xml_str: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    let mut event_reader = Reader::from_str(&xml_str);
    event_reader.check_end_names(false);
    match event_reader.read_event()? {
        Event::Start(e) => {
            if e.name().as_ref() == b"wd:re" {
                let parsed = script.parse(worker, &mut event_reader, b"", include_adaptor)?;
                return run_xml(
                    script,
                    "<r>".to_owned() + std::str::from_utf8(&parsed)? + "</r>",
                    worker,
                    include_adaptor,
                );
            }
        }
        _ => {}
    }
    Ok(vec![])
}

pub(super) fn r#if_xml_parser<T: IncludeAdaptor>(
    script: &mut Script,
    attributes: &HashMap<(String, String), String>,
    xml_str: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    if crate::attr_parse_or_static_xml_parser(worker, attributes, "value") == b"true" {
        let mut tokenizer = Tokenizer::from(xml_str);
        if let Some(Ok(Token::ElementStart { prefix, local, .. })) = tokenizer.next() {
            if prefix.as_str() == "wd" && local.as_str() == "if" {
                return script.parse_xml_parser(worker, &mut tokenizer, ("", ""), include_adaptor);
            }
        }
    }
    Ok(vec![])
}

pub(super) fn r#if<T: IncludeAdaptor>(
    script: &mut Script,
    e: &BytesStart,
    xml_str: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    if crate::attr_parse_or_static(worker, &xml_util::attr2hash_map(&e), "value") == b"true" {
        let mut event_reader = Reader::from_str(&xml_str);
        event_reader.check_end_names(false);
        if let Event::Start(e) = event_reader.read_event()? {
            if e.name().as_ref() == b"wd:if" {
                return script.parse(worker, &mut event_reader, b"", include_adaptor);
            }
        }
    }
    Ok(vec![])
}
pub(super) fn r#for<T: IncludeAdaptor>(
    script: &mut Script,
    e: &BytesStart,
    xml_str: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    let mut r = Vec::new();
    let attr = xml_util::attr2hash_map(&e);
    let var = crate::attr_parse_or_static_string(worker, &attr, "var");
    if var != "" {
        if let Some(source) = attr.get("wd:in") {
            let source = std::str::from_utf8(source)?;
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
                    match ev.read_event()? {
                        Event::Start(e) => {
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
                                    + (if let Some(index) = e.try_get_attribute(b"index")? {
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
    Ok(r)
}

pub(super) fn r#for_xml_parser<T: IncludeAdaptor>(
    script: &mut Script,
    attributes: &HashMap<(String, String), String>,
    xml: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>> {
    let mut r = Vec::new();
    let var = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "var");
    if var != "" {
        if let Some(source) = attributes.get(&("wd".to_owned(), "in".to_owned())) {
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
                let mut tokenizer = Tokenizer::from(xml); //TODO:使いまわしできないか後で試す
                while let Some(Ok(token)) = tokenizer.next() {
                    match token {
                        Token::ElementStart { prefix, local, .. } => {
                            let (attributes_str, attributes) = xml_util::attributes(&mut tokenizer);
                            if prefix.as_str() == "wd" && local.as_str() == "for" {
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
                                    + (if let Some(index) =
                                        attributes.get(&("".to_owned(), "index".to_owned()))
                                    {
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
                                    let scope = &mut worker.js_runtime.handle_scope();
                                    let context = scope.get_current_context();
                                    let scope = &mut v8::ContextScope::new(scope, context);
                                    v8::String::new(scope, &source)
                                        .and_then(|code| v8::Script::compile(scope, code, None))
                                        .and_then(|v| v.run(scope));
                                }
                                r.append(&mut script.parse_xml_parser(
                                    worker,
                                    &mut tokenizer,
                                    ("wd", "for"),
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
    Ok(r)
}
