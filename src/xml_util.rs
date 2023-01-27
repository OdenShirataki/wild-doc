use quick_xml::{events::Event, name::QName, Reader, Writer};
use rustc_hash::FxHasher;
use std::{borrow::Cow, collections::HashMap, hash::BuildHasherDefault, io::Cursor};

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

pub fn outer(elem: &Event, name: QName, xml_reader: &mut Reader<&[u8]>) -> String {
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
