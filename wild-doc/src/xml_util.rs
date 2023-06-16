use maybe_xml::scanner::{Scanner, State};

pub(crate) fn inner<'a>(xml: &'a [u8]) -> (&[u8], usize) {
    let mut pos = 0;
    let mut deps = 0;
    let mut scanner = Scanner::new();
    while let Some(state) = scanner.scan(&xml[pos..]) {
        match state {
            State::ScannedStartTag(end) => {
                deps += 1;
                pos += end;
            }
            State::ScannedEndTag(end) => {
                deps -= 1;
                if deps < 0 {
                    return (&xml[..pos], pos + end);
                }
                pos += end;
            }
            State::ScannedProcessingInstruction(end)
            | State::ScannedCharacters(end)
            | State::ScannedCdata(end)
            | State::ScannedComment(end)
            | State::ScannedDeclaration(end)
            | State::ScannedEmptyElementTag(end) => pos += end,
            _ => {}
        }
    }
    (&xml[..0], 0)
}
