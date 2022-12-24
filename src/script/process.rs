use deno_runtime::{deno_core::v8, worker::MainWorker};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use std::convert::TryFrom;

use crate::{xml_util, IncludeAdaptor};

use super::Script;

pub(super) fn case<T: IncludeAdaptor>(
    script: &mut Script,
    e: &BytesStart,
    xml_str: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>, std::io::Error> {
    let mut r = Vec::new();
    let attr = xml_util::attr2hash_map(&e);
    let cmp_value = crate::attr_parse_or_static(worker, &attr, "value");
    if cmp_value != b"" {
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
                                                let attr = xml_util::attr2hash_map(&e);
                                                let wv = crate::attr_parse_or_static(
                                                    worker, &attr, "value",
                                                );
                                                if wv == cmp_value {
                                                    let xml_str = xml_util::outer(
                                                        &next,
                                                        name,
                                                        &mut event_reader,
                                                    );
                                                    let mut event_reader_inner =
                                                        Reader::from_str(&xml_str.trim());
                                                    event_reader_inner.check_end_names(false);
                                                    loop {
                                                        match event_reader_inner.read_event() {
                                                            Ok(Event::Start(e)) => {
                                                                if e.name().as_ref() == b"wd:when" {
                                                                    r.append(&mut script.parse(
                                                                        worker,
                                                                        &mut event_reader_inner,
                                                                        "",
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
                                                                    "",
                                                                    include_adaptor,
                                                                )?);
                                                                break;
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                            _ => {}
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

pub(super) fn r#for<T: IncludeAdaptor>(
    script: &mut Script,
    e: &BytesStart,
    xml_str: &str,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
) -> Result<Vec<u8>, std::io::Error> {
    let mut r = Vec::new();
    let attr = xml_util::attr2hash_map(&e);
    let var = crate::attr_parse_or_static_string(worker, &attr, "var");
    if var != "" {
        if let Some(arr) = attr.get("wd:in") {
            if let Ok(arr) = std::str::from_utf8(arr) {
                let rs = {
                    let scope = &mut worker.js_runtime.handle_scope();
                    let context = scope.get_current_context();
                    let scope = &mut v8::ContextScope::new(scope, context);
                    v8::String::new(scope, &arr)
                        .and_then(|code| v8::Script::compile(scope, code, None))
                        .and_then(|code| code.run(scope))
                        .and_then(|v| v8::Local::<v8::Array>::try_from(v).ok())
                };
                if let Some(rs) = rs {
                    let length = rs.length();
                    for i in 0..length {
                        let mut ev = Reader::from_str(&xml_str);
                        ev.check_end_names(false);
                        loop {
                            match ev.read_event() {
                                Ok(Event::Start(e)) => {
                                    if e.name().as_ref() == b"wd:for" {
                                        {
                                            let scope = &mut worker.js_runtime.handle_scope();
                                            let context = scope.get_current_context();
                                            let scope = &mut v8::ContextScope::new(scope, context);
                                            v8::String::new(
                                                scope,
                                                &("wd.stack.push({".to_owned()
                                                    + &var.to_string()
                                                    + ":"
                                                    + arr
                                                    + "["
                                                    + &i.to_string()
                                                    + "]"
                                                    + &(if let Ok(Some(index)) =
                                                        e.try_get_attribute(b"index")
                                                    {
                                                        std::str::from_utf8(&index.value).map_or(
                                                            "".to_string(),
                                                            |v| {
                                                                ",".to_owned()
                                                                    + v
                                                                    + ":"
                                                                    + &i.to_string()
                                                            },
                                                        )
                                                    } else {
                                                        "".to_owned()
                                                    })
                                                    + "})"),
                                            )
                                            .and_then(|code| v8::Script::compile(scope, code, None))
                                            .and_then(|v| v.run(scope));
                                        }
                                        r.append(&mut script.parse(
                                            worker,
                                            &mut ev,
                                            "wd:for",
                                            include_adaptor,
                                        )?);
                                        let _ =
                                            worker.execute_script("pop stack", "wd.stack.pop()");

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
    }
    Ok(r)
}
