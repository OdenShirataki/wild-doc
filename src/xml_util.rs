use quick_xml::{events::Event, name::QName, Reader, Writer};
use rustc_hash::FxHasher;
use std::{borrow::Cow, collections::HashMap, hash::BuildHasherDefault, io::Cursor};
use xmlparser::{ElementEnd, Token, Tokenizer};

pub type XmlAttr<'a> = HashMap<String, Cow<'a, [u8]>, BuildHasherDefault<FxHasher>>;

pub fn attr2hash_map<'a>(e: &'a quick_xml::events::BytesStart<'a>) -> XmlAttr {
    let mut m: XmlAttr = HashMap::default();
    for a in e.html_attributes() {
        if let Ok(a) = a {
            if let Ok(key) = std::str::from_utf8(a.key.as_ref()) {
                m.insert(key.into(), a.value);
            }
        }
    }
    m
}

pub(crate) fn outer_xml_parser(
    start: &str,
    start_attributes: &str,
    outer_prefix: &str,
    outer_local: &str,
    tokenizer: &mut xmlparser::Tokenizer,
) -> String {
    println!("outer_xml_parser");
    let mut r = start.to_string();
    if start_attributes.len() > 0 {
        r += " ";
        r += start_attributes;
    }
    r += ">";
    let mut deps = 0;
    while let Some(Ok(token)) = tokenizer.next() {
        println!("{:?}",token);
        match token {
            Token::ElementStart {
                prefix,
                local,
                span,
            } => {
                if prefix.as_str() == outer_prefix && local.as_str() == outer_local {
                    deps += 1;
                }
                r += span.as_str();
            }
            Token::Attribute { span, .. } => {
                r += " ";
                r += span.as_str();
            }
            Token::ElementEnd { end, span } => {
                if let ElementEnd::Close(prefix, local) = end {
                    if prefix.as_str() == outer_prefix && local.as_str() == outer_local {
                        deps -= 1;
                    }
                }
                r += span.as_str();
            }
            _ => r += token_str(&token),
        }
        if deps < 0 {
            break;
        }
    }
    r
}

fn token_str<'a>(token: &'a Token) -> &'a str {
    match token {
        Token::Declaration { span, .. }
        | Token::ProcessingInstruction { span, .. }
        | Token::Comment { span, .. }
        | Token::DtdStart { span, .. }
        | Token::EmptyDtd { span, .. }
        | Token::EntityDeclaration { span, .. }
        | Token::DtdEnd { span, .. }
        | Token::Attribute { span, .. }
        | Token::Cdata { span, .. } => {
            return span.as_str();
        }
        Token::Text { text } => {
            return text.as_str();
        }
        _ => {}
    }
    ""
}
pub(crate) fn inner_xml_parser(
    outer_prefix: &str,
    outer_local: &str,
    tokenizer: &mut xmlparser::Tokenizer,
) -> String {
    let mut r = String::new();
    let mut deps = 0;
    while let Some(Ok(token)) = tokenizer.next() {
        match token {
            Token::ElementStart {
                prefix,
                local,
                span,
            } => {
                if prefix.as_str() == outer_prefix && local.as_str() == outer_local {
                    deps += 1;
                }
                r += span.as_str();
            }
            Token::Attribute { span, .. } => {
                r += " ";
                r += span.as_str();
            }
            Token::ElementEnd { end, span } => {
                if let ElementEnd::Close(prefix, local) = end {
                    if prefix.as_str() == outer_prefix && local.as_str() == outer_local {
                        deps -= 1;
                        if deps < 0 {
                            break;
                        }
                    }
                }
                r += span.as_str();
            }
            _ => r += token_str(&token),
        }
    }
    r
}

pub(crate) fn outer(elem: &Event, name: QName, xml_reader: &mut Reader<&[u8]>) -> String {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_event(elem).unwrap();
    let mut deps = 0;
    loop {
        if let Ok(e) = xml_reader.read_event() {
            match e {
                Event::Start(ref e) => {
                    if e.name() == name {
                        deps += 1;
                    }
                }
                Event::End(ref e) => {
                    if e.name() == name {
                        deps -= 1;
                    }
                }
                Event::Eof => {
                    break;
                }
                _ => {}
            }
            let _ = writer.write_event(e);
            if deps < 0 {
                break;
            }
        }
    }
    std::str::from_utf8(writer.into_inner().get_ref()).map_or("".to_string(), |v| v.to_string())
}

pub(crate) fn attributes(
    tokeninzer: &mut Tokenizer,
) -> (String, HashMap<(String, String), String>) {
    let mut str_span = String::new();
    let mut attributes = HashMap::new();
    while let Some(Ok(token)) = tokeninzer.next() {
        match token {
            Token::Attribute {
                prefix,
                local,
                value,
                span,
            } => {
                if str_span.len() > 0 {
                    str_span += " ";
                }
                str_span += span.as_str();
                attributes.insert((prefix.to_string(), local.to_string()), value.to_string());
            }
            Token::ElementEnd { .. } => {
                break;
            }
            _ => {}
        }
    }
    (str_span, attributes)
}
