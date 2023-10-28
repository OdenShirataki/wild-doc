use hashbrown::HashMap;
use maybe_xml::{
    scanner::{Scanner, State},
    token,
};
use semilattice_database_session::search::{Join, JoinCondition};

use crate::parser::{AttributeMap, Parser};

impl Parser {
    pub async fn join<'a>(
        &mut self,
        xml: &'a [u8],
        attributes: &AttributeMap,
        search_map: &mut HashMap<String, Join>,
    ) -> &'a [u8] {
        if let Some(Some(name)) = attributes.get("name") {
            let name = name.to_str();
            if name != "" {
                if let Some(collection_id) = self.collection_id(attributes) {
                    let (last_xml, condition) = self.join_condition_loop(xml).await;
                    search_map.insert(name.into(), Join::new(collection_id, condition));
                    return last_xml;
                }
            }
        }
        return xml;
    }

    async fn join_condition_loop<'a>(&mut self, xml: &'a [u8]) -> (&'a [u8], Vec<JoinCondition>) {
        let mut xml = xml;
        let mut scanner = Scanner::new();
        let mut futs = vec![];

        while let Some(state) = scanner.scan(xml) {
            match state {
                State::ScannedEmptyElementTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::EmptyElementTag::from(token_bytes);
                    let name = token.name();
                    match name.local().as_bytes() {
                        b"pends" => {
                            futs.push(Self::join_condition_pends(
                                self.parse_attibutes(token.attributes()).await,
                            ));
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

    async fn join_condition_pends(attributes: AttributeMap) -> JoinCondition {
        JoinCondition::Pends {
            key: attributes
                .get("key")
                .and_then(|v| v.as_ref())
                .map(|v| v.to_str().into()),
        }
    }
}
