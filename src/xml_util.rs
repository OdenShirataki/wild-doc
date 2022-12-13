use quick_xml::name::QName;
use quick_xml::{events::Event, Reader, Writer};
use rustc_hash::FxHasher;
use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::io::Cursor;

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
pub fn inner(xml_reader: &mut Reader<&[u8]>) -> String {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut pm = 0;
    loop {
        if let Ok(e) = xml_reader.read_event() {
            match e {
                Event::Start(_) => {
                    pm += 1;
                }
                Event::End(_) => {
                    pm -= 1;
                }
                _ => {}
            }
            let _ = writer.write_event(e);
            if pm <= 0 {
                break;
            }
        }
    }
    std::str::from_utf8(writer.into_inner().get_ref()).map_or("".to_string(), |v| v.to_string())
}
pub fn outer(elem: &Event, xml_reader: &mut Reader<&[u8]>) -> String {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_event(elem).unwrap();
    let mut pm = 1;
    loop {
        if let Ok(e) = xml_reader.read_event() {
            match e {
                Event::Start(_) => {
                    pm += 1;
                }
                Event::End(_) => {
                    pm -= 1;
                }
                _ => {}
            }
            let _ = writer.write_event(e);
            if pm <= 0 {
                break;
            }
        }
    }
    std::str::from_utf8(writer.into_inner().get_ref()).map_or("".to_string(), |v| v.to_string())
}

pub fn text_content(xml_reader: &mut Reader<&[u8]>, tag: QName) -> String {
    let mut cont = "".to_string();
    loop {
        match xml_reader.read_event() {
            Ok(Event::End(e)) => {
                if e.name() == tag {
                    break;
                }
            }
            Ok(Event::CData(cdata)) => {
                cont = std::str::from_utf8(&cdata).unwrap_or("").to_string();
            }
            Ok(Event::Text(txt)) => {
                cont = std::str::from_utf8(&txt).unwrap_or("").to_string();
            }
            _ => {}
        }
    }
    cont
}
