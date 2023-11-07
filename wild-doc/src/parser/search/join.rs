use hashbrown::HashMap;
use maybe_xml::{token::Ty, Lexer};
use semilattice_database_session::search::{Join, JoinCondition};
use wild_doc_script::Vars;

use crate::parser::Parser;

impl Parser {
    pub async fn join(
        &self,
        xml: &[u8],
        pos: &mut usize,
        attr: &Vars,
        search_map: &mut HashMap<String, Join>,
    ) {
        if let Some(name) = attr.get("name") {
            let name = name.to_str();
            if name != "" {
                if let Some(collection_id) = self.collection_id(attr) {
                    let condition = self.join_condition_loop(xml, pos).await;
                    search_map.insert(name.into(), Join::new(collection_id, condition));
                }
            }
        }
    }

    async fn join_condition_loop(&self, xml: &[u8], pos: &mut usize) -> Vec<JoinCondition> {
        let mut futs = vec![];
        let lexer = unsafe { Lexer::from_slice_unchecked(xml) };
        while let Some(token) = lexer.tokenize(pos) {
            match token.ty() {
                Ty::EmptyElementTag(eet) => match eet.name().local().as_bytes() {
                    b"pends" => {
                        let vars = self.vars_from_attibutes(eet.attributes()).await;
                        futs.push(async move {
                            JoinCondition::Pends {
                                key: vars.get("key").map(|v| v.to_str().into()),
                            }
                        });
                    }
                    _ => {}
                },
                Ty::EndTag(et) => match et.name().as_bytes() {
                    b"join" => {
                        break;
                    }
                    _ => {}
                },
                Ty::StartTag(_)
                | Ty::Characters(_)
                | Ty::Cdata(_)
                | Ty::Comment(_)
                | Ty::Declaration(_)
                | Ty::ProcessingInstruction(_) => {}
            }
        }
        futures::future::join_all(futs).await.into_iter().collect()
    }
}
