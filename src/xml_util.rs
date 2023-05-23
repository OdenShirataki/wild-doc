use std::io::BufReader;

fn token2vec(token: &maybe_xml::token::owned::Token) -> Vec<u8> {
    let mut r = vec![];
    match token {
        maybe_xml::token::owned::Token::Cdata(cdata) => {
            r.append(&mut cdata.content().to_vec());
        }
        maybe_xml::token::owned::Token::EmptyElementTag(val) => {
            r.append(&mut val.to_vec());
        }
        maybe_xml::token::owned::Token::Characters(val) => {
            r.append(&mut val.to_vec());
        }
        maybe_xml::token::owned::Token::ProcessingInstruction(val) => {
            r.append(&mut val.to_vec());
        }
        maybe_xml::token::owned::Token::Declaration(val) => {
            r.append(&mut val.to_vec());
        }
        maybe_xml::token::owned::Token::Comment(val) => {
            r.append(&mut val.to_vec());
        }
        maybe_xml::token::owned::Token::EofWithBytesNotEvaluated(val) => {
            r.append(&mut val.to_vec());
        }
        _ => {}
    }
    r
}

pub(crate) fn inner(
    outer: &maybe_xml::token::prop::TagName,
    tokenizer: &mut maybe_xml::eval::bufread::IntoIter<BufReader<&[u8]>>,
) -> Vec<u8> {
    let mut r = Vec::new();
    let mut deps = 0;
    while let Some(token) = tokenizer.next() {
        match token {
            maybe_xml::token::owned::Token::StartTag(tag) => {
                if &tag.name() == outer {
                    deps += 1;
                }
                r.append(&mut tag.to_vec());
            }
            maybe_xml::token::owned::Token::EndTag(tag) => {
                if &tag.name() == outer {
                    deps -= 1;
                    if deps < 0 {
                        break;
                    }
                }
                r.append(&mut tag.to_vec());
            }
            _ => {
                r.append(&mut token2vec(&token));
            }
        }
    }
    r
}
