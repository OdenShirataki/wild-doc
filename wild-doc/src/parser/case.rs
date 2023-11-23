use anyhow::Result;
use maybe_xml::{token::Ty, Reader};
use wild_doc_script::Vars;

use crate::xml_util;

use super::Parser;

impl Parser {
    pub(super) async fn case(
        &mut self,
        xml: &[u8],
        pos: &mut usize,
        attr: Vars,
    ) -> Result<Vec<u8>> {
        let mut r = None;

        let cmp_src = attr.get("value");
        let reader = Reader::from_str(unsafe { std::str::from_utf8_unchecked(xml) });
        while let Some(token) = reader.tokenize(pos) {
            match token.ty() {
                Ty::StartTag(st) => {
                    let name = st.name();
                    match name.as_bytes() {
                        b"wd:when" => {
                            if let Some(right) =
                                self.vars_from_attibutes(st.attributes()).await.get("value")
                            {
                                if let Some(cmp_src) = cmp_src {
                                    if cmp_src == right {
                                        r = Some(self.parse(xml, pos).await?);
                                    }
                                }
                            }
                            if !r.is_some() {
                                xml_util::to_end(xml, pos);
                            }
                        }
                        b"wd:else" => {
                            if r.is_some() {
                                xml_util::to_end(xml, pos);
                            } else {
                                r = Some(self.parse(xml, pos).await?)
                            }
                        }
                        _ => {}
                    }
                }
                Ty::EmptyElementTag(_)
                | Ty::EndTag(_)
                | Ty::Characters(_)
                | Ty::Cdata(_)
                | Ty::Comment(_)
                | Ty::Declaration(_) => {}
                _ => {
                    break;
                }
            }
        }
        Ok(r.unwrap_or_default())
    }
}
