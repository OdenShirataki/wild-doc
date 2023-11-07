use maybe_xml::{token::Ty, Lexer};

#[inline(always)]
pub(crate) fn to_end(xml: &[u8], pos: &mut usize) -> (usize, usize) {
    let mut pos_before = *pos;
    let mut deps = 0;
    let lexer = unsafe { Lexer::from_slice_unchecked(xml) };
    while let Some(token) = lexer.tokenize(pos) {
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

#[inline(always)]
pub(crate) fn quot_unescape(value: &str) -> String {
    value.replace("&#039;", "'").replace("&quot;", "\"")
}

#[inline(always)]
pub(crate) fn escape_html(s: &str) -> String {
    s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
}
