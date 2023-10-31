use hashbrown::HashMap;
use maybe_xml::{
    scanner::{Scanner, State},
    token,
};
use semilattice_database_session::search::{Join, JoinCondition};
use wild_doc_script::Vars;

use crate::parser::Parser;

impl Parser {
    pub async fn join<'a>(
        &mut self,
        xml: &'a [u8],
        vars: &Vars,
        search_map: &mut HashMap<String, Join>,
        stack: &Vars,
    ) -> &'a [u8] {
        if let Some(name) = vars.get("name") {
            let name = name.to_str();
            if name != "" {
                if let Some(collection_id) = self.collection_id(vars) {
                    let (last_xml, condition) = self.join_condition_loop(xml, stack).await;
                    search_map.insert(name.into(), Join::new(collection_id, condition));
                    return last_xml;
                }
            }
        }
        return xml;
    }

    async fn join_condition_loop<'a>(
        &mut self,
        xml: &'a [u8],
        stack: &Vars,
    ) -> (&'a [u8], Vec<JoinCondition>) {
        let mut xml = xml;
        let mut scanner = Scanner::new();
        let mut futs = vec![];

        while let Some(state) = scanner.scan(xml) {
            match state {
                State::ScannedEmptyElementTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::EmptyElementTag::from(token_bytes);
                    match token.name().local().as_bytes() {
                        b"pends" => {
                            let vars = self.vars_from_attibutes(token.attributes(), stack).await;
                            futs.push(async move {
                                JoinCondition::Pends {
                                    key: vars.get("key").map(|v| v.to_str().into()),
                                }
                            });
                        }
                        _ => {}
                    }
                }
                State::ScannedEndTag(pos) => {
                    let token = token::EndTag::from(&xml[..pos]);
                    xml = &xml[pos..];
                    match token.name().as_bytes() {
                        b"join" => {
                            break;
                        }
                        _ => {}
                    }
                }
                State::ScannedStartTag(pos)
                | State::ScannedCharacters(pos)
                | State::ScannedCdata(pos)
                | State::ScannedComment(pos)
                | State::ScannedDeclaration(pos)
                | State::ScannedProcessingInstruction(pos) => {
                    xml = &xml[pos..];
                }
                _ => {}
            }
        }
        (
            xml,
            futures::future::join_all(futs).await.into_iter().collect(),
        )
    }
}
