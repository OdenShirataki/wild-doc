use hashbrown::HashMap;
use maybe_xml::{token::Ty, Lexer};
use semilattice_database_session::search::{Join, JoinCondition};
use wild_doc_script::Vars;

use crate::parser::Parser;

impl Parser {
    pub async fn join(
        &mut self,
        lexer: &Lexer<'_>,
        pos: &mut usize,
        vars: &Vars,
        search_map: &mut HashMap<String, Join>,
        stack: &Vars,
    ) {
        if let Some(name) = vars.get("name") {
            let name = name.to_str();
            if name != "" {
                if let Some(collection_id) = self.collection_id(vars) {
                    let condition = self.join_condition_loop(lexer, pos, stack).await;
                    search_map.insert(name.into(), Join::new(collection_id, condition));
                }
            }
        }
    }

    async fn join_condition_loop(
        &mut self,
        lexer: &Lexer<'_>,
        pos: &mut usize,
        stack: &Vars,
    ) -> Vec<JoinCondition> {
        let mut futs = vec![];

        while let Some(token) = lexer.tokenize(pos) {
            match token.ty() {
                Ty::EmptyElementTag(eet) => match eet.name().local().as_bytes() {
                    b"pends" => {
                        let vars = self.vars_from_attibutes(eet.attributes(), stack).await;
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
