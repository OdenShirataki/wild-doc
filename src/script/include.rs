use deno_runtime::worker::MainWorker;

use crate::{xml_util::XmlAttr, IncludeAdaptor};

pub fn get_include_content<T: IncludeAdaptor>(
    script: &mut super::Script,
    worker: &mut MainWorker,
    include_adaptor: &mut T,
    attr: &XmlAttr,
) -> std::io::Result<Vec<u8>> {
    let mut r = Vec::new();
    let xml = if let Some(xml) =
        include_adaptor.include(&crate::attr_parse_or_static_string(worker, attr, "src"))
    {
        Some(xml)
    } else {
        let substitute = crate::attr_parse_or_static_string(worker, attr, "substitute");
        if let Some(xml) = include_adaptor.include(&substitute) {
            Some(xml)
        } else {
            None
        }
    };
    if let Some(xml) = xml {
        if xml.len() > 0 {
            let str_xml = "<root>".to_owned() + &xml + "</root>";
            let mut event_reader_inner = quick_xml::Reader::from_str(&str_xml);
            event_reader_inner.check_end_names(false);
            loop {
                match event_reader_inner.read_event() {
                    Ok(quick_xml::events::Event::Start(e)) => {
                        if e.name().as_ref() == b"root" {
                            r.append(&mut script.parse(
                                worker,
                                &mut event_reader_inner,
                                "root",
                                include_adaptor,
                            )?);
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(r)
}
