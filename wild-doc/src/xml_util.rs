use maybe_xml::{Reader, token::Ty};

pub(crate) fn to_end(xml: &[u8], pos: &mut usize) -> (usize, usize) {
    let mut pos_before = *pos;
    let mut deps = 0;
    let reader = Reader::from_str(crate::xml_util::valid_utf8_prefix(xml));
    while let Some(token) = reader.tokenize(pos) {
        match token.ty() {
            Ty::StartTag(_) => {
                deps += 1;
            }
            Ty::EndTag(_) => {
                deps -= 1;
                if deps < 0 {
                    return (pos_before, *pos);
                }
            }
            Ty::ProcessingInstruction(_)
            | Ty::Characters(_)
            | Ty::Cdata(_)
            | Ty::Comment(_)
            | Ty::Declaration(_)
            | Ty::EmptyElementTag(_) => {}
        }
        pos_before = *pos;
    }
    (0, 0)
}

pub(crate) fn quot_unescape(value: &str) -> String {
    value.replace("&#039;", "'").replace("&quot;", "\"")
}

pub(crate) fn escape_html(s: &str) -> String {
    s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
}

/// Returns the longest prefix of `xml` that is valid UTF-8. Byte positions
/// stay the same as in `xml`, which a lossy conversion would not keep.
pub(crate) fn valid_utf8_prefix(xml: &[u8]) -> &str {
    match std::str::from_utf8(xml) {
        Ok(s) => s,
        Err(e) => std::str::from_utf8(&xml[..e.valid_up_to()]).unwrap_or_default(),
    }
}
