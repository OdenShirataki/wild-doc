use anyhow::Result;
use maybe_xml::{token::Ty, Reader};
use wild_doc_script::Vars;

use crate::{r#const::*, xml_util};

use super::Parser;

impl Parser {
    pub(super) async fn case(
        &mut self,
        xml: &[u8],
        pos: &mut usize,
        attr: Vars,
    ) -> Result<Vec<u8>> {
        let mut r = None;

        let cmp_src = attr.get(&*VALUE);
        let reader = Reader::from_str(unsafe { std::str::from_utf8_unchecked(xml) });
        while let Some(token) = reader.tokenize(pos) {
            match token.ty() {
                Ty::StartTag(st) => {
                    let name = st.name();
                    match name.as_bytes() {
                        b"wd:when" => {
                            if let Some(right) =
                                self.vars_from_attibutes(st.attributes()).await.get(&*VALUE)
                            {
                                if let Some(cmp_src) = cmp_src {
                                    if cmp_src == right {
                                        r = Some(self.parse(xml, pos).await?);
                                    }
                                }
                            }
                            if r.is_none() {
                                xml_util::to_end(xml, pos);
                            }
                        }
                        b"wd:else" => {
                            if r.is_none() {
                                r = Some(self.parse(xml, pos).await?)
                            } else {
                                xml_util::to_end(xml, pos);
                            }
                        }
                        _ => {}
                    }
                }
                Ty::EndTag(t) => {
                    if t.name().as_str() == "wd:case" {
                        break;
                    }
                }
                _ => {}
            }
        }
        Ok(r.unwrap_or_default())
    }
}
