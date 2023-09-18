use std::collections::HashMap;

use maybe_xml::{
    scanner::{Scanner, State},
    token,
};
use semilattice_database_session::search::{Join, JoinCondition};

use crate::parser::{AttributeMap, Parser};

impl Parser {
    #[inline(always)]
    pub fn join<'a>(
        &mut self,
        xml: &'a [u8],
        attributes: &AttributeMap,
        search_map: &mut HashMap<String, Join>,
    ) -> &'a [u8] {
        if let Some(Some(name)) = attributes.get(b"name".as_ref()) {
            if let Some(name) = name.as_str() {
                if name != "" {
                    if let Some(collection_id) = self.collection_id(attributes) {
                        let (last_xml, condition) = self.join_condition_loop(xml);
                        search_map.insert(name.to_owned(), Join::new(collection_id, condition));
                        return last_xml;
                    }
                }
            }
        }
        return xml;
    }

    #[inline(always)]
    fn join_condition_loop<'a>(&mut self, xml: &'a [u8]) -> (&'a [u8], Vec<JoinCondition>) {
        let mut result_conditions = Vec::new();
        let mut xml = xml;
        let mut scanner = Scanner::new();
        while let Some(state) = scanner.scan(xml) {
            match state {
                State::ScannedStartTag(pos) => {
                    xml = &xml[pos..];
                }
                State::ScannedEmptyElementTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::EmptyElementTag::from(token_bytes);
                    let attributes = self.parse_attibutes(&token.attributes());
                    let name = token.name();
                    match name.local().as_bytes() {
                        b"pends" => {
                            result_conditions.push(Self::join_condition_pends(&attributes));
                        }
                        _ => {}
                    }
                }
                State::ScannedEndTag(pos) => {
                    let token = token::borrowed::EndTag::from(&xml[..pos]);
                    xml = &xml[pos..];
                    match token.name().as_bytes() {
                        b"join" => {
                            break;
                        }
                        _ => {}
                    }
                }
                State::ScannedCharacters(pos)
                | State::ScannedCdata(pos)
                | State::ScannedComment(pos)
                | State::ScannedDeclaration(pos)
                | State::ScannedProcessingInstruction(pos) => {
                    xml = &xml[pos..];
                }
                _ => {}
            }
        }
        (xml, result_conditions)
    }

    #[inline(always)]
    fn join_condition_pends(attributes: &AttributeMap) -> JoinCondition {
        JoinCondition::Pends {
            key: attributes
                .get(b"key".as_ref())
                .and_then(|v| v.as_ref())
                .map(|v| v.as_str().unwrap_or("").to_owned()),
        }
    }
}
