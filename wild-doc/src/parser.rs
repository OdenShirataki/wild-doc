mod attr;
mod collection;
mod process;
mod record;
mod result;
mod search;
mod session;
mod update;
mod var;

use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, Mutex, RwLock},
};

use anyhow::Result;

use bson::Bson;
use maybe_xml::{
    scanner::{Scanner, State},
    token::{
        self,
        prop::{Attributes, TagName},
    },
};
use semilattice_database_session::{Session, SessionDatabase};
use wild_doc_script::{WildDocScript, WildDocState};

use crate::xml_util;

type AttributeMap = HashMap<Vec<u8>, Option<Arc<Bson>>>;

struct SessionState {
    session: Session,
    commit_on_close: bool,
    clear_on_close: bool,
}
pub struct Parser {
    database: Arc<RwLock<SessionDatabase>>,
    sessions: Vec<SessionState>,
    scripts: Arc<HashMap<String, Arc<Mutex<dyn WildDocScript>>>>,
    state: WildDocState,
    include_stack: Vec<String>,
}
impl Parser {
    pub fn new(
        database: Arc<RwLock<SessionDatabase>>,
        scripts: Arc<HashMap<String, Arc<Mutex<dyn WildDocScript>>>>,
        state: WildDocState,
    ) -> Result<Self> {
        Ok(Self {
            scripts,
            sessions: vec![],
            database,
            state,
            include_stack: vec![],
        })
    }

    #[inline(always)]
    fn parse_wd_start_or_empty_tag(
        &mut self,
        name: &[u8],
        attributes: &Option<Attributes<'_>>,
    ) -> Result<Option<Vec<u8>>> {
        match name {
            b"print" => {
                return Ok(self
                    .parse_attibutes(attributes)
                    .get(b"value".as_ref())
                    .and_then(|v| v.as_ref())
                    .map(|v| match v.deref() {
                        Bson::Binary(v) => v.as_raw_binary().bytes.to_vec(),
                        Bson::String(v) => v.to_owned().into_bytes(),
                        _ => v.to_string().into_bytes(),
                    }));
            }
            b"global" => {
                let attributes = self.parse_attibutes(attributes);
                if let (Some(Some(var)), Some(Some(value))) = (
                    attributes.get(b"var".as_ref()),
                    attributes.get(b"value".as_ref()),
                ) {
                    if let Some(var) = var.as_str() {
                        self.register_global(var, value);
                    }
                }
            }
            b"print_escape_html" => {
                return Ok(self
                    .parse_attibutes(attributes)
                    .get(b"value".as_ref())
                    .and_then(|v| v.as_ref())
                    .map(|v| xml_util::escape_html(v.as_str().map_or("", |v| v)).into_bytes()));
            }
            b"include" => {
                let attributes = self.parse_attibutes(attributes);
                return Ok(Some(self.get_include_content(attributes)?));
            }
            b"delete_collection" => {
                let attributes = self.parse_attibutes(attributes);
                self.delete_collection(attributes);
            }
            b"session_gc" => {
                let attributes = self.parse_attibutes(attributes);
                self.session_gc(attributes);
            }
            _ => {}
        }
        Ok(None)
    }

    #[inline(always)]
    fn is_wd_tag(name: &TagName) -> bool {
        name.namespace_prefix()
            .map_or(false, |v| v.as_bytes() == b"wd")
    }

    #[inline(always)]
    pub fn parse(&mut self, xml: &[u8]) -> Result<Vec<u8>> {
        let mut r: Vec<u8> = Vec::new();
        let mut tag_stack = vec![];
        let mut search_map = HashMap::new();
        let mut xml = xml;

        let mut scanner = Scanner::new();

        while let Some(state) = scanner.scan(&xml) {
            match state {
                State::ScannedProcessingInstruction(pos) => {
                    let token_bytes = &xml[..pos];
                    let token = token::borrowed::ProcessingInstruction::from(token_bytes);
                    let target = token.target();
                    if let Some(script) = self.scripts.get(target.to_str()?) {
                        if let Some(i) = token.instructions() {
                            if let Err(e) = script.lock().unwrap().evaluate_module(
                                self.include_stack.last().map_or("", |v| v),
                                i.as_bytes(),
                            ) {
                                return Err(e);
                            }
                        }
                    } else {
                        r.extend(token_bytes.to_vec());
                    }
                    xml = &xml[pos..];
                }
                State::ScannedStartTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::StartTag::from(token_bytes);
                    let name = token.name();
                    if Self::is_wd_tag(&name) {
                        if let Some(parsed) = self.parse_wd_start_or_empty_tag(
                            name.local().as_bytes(),
                            &token.attributes(),
                        )? {
                            r.extend(parsed);
                        } else {
                            match name.local().as_bytes() {
                                b"session" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.session(attributes);
                                }
                                b"session_sequence_cursor" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.session_sequence(attributes);
                                }
                                b"sessions" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.sessions(&attributes);
                                }
                                b"re" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let parsed = self.parse(inner_xml)?;
                                    xml = &xml[outer_end..];
                                    r.extend(self.parse(&parsed)?);
                                }
                                b"comment" => {
                                    let (_, outer_end) = xml_util::inner(xml);
                                    xml = &xml[outer_end..];
                                }
                                b"letitgo" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.extend(inner_xml.to_vec());
                                    xml = &xml[outer_end..];
                                }
                                b"update" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    self.update(inner_xml, &attributes)?;
                                    xml = &xml[outer_end..];
                                }
                                b"search" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    xml = self.search(xml, &attributes, &mut search_map);
                                }
                                b"result" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.result(&attributes, &search_map);
                                }
                                b"record" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.record(attributes);
                                }
                                b"collections" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.collections(attributes);
                                }
                                b"case" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.extend(self.case(attributes, inner_xml)?);
                                    xml = &xml[outer_end..];
                                }
                                b"if" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.extend(self.r#if(attributes, inner_xml)?);
                                    xml = &xml[outer_end..];
                                }
                                b"for" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.extend(self.r#for(attributes, inner_xml)?);
                                    xml = &xml[outer_end..];
                                }
                                b"while" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.extend(self.r#while(token.attributes(), inner_xml)?);
                                    xml = &xml[outer_end..];
                                }
                                b"tag" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    let (name, attr) = self.custom_tag(attributes);
                                    tag_stack.push(name.clone());
                                    r.push(b'<');
                                    r.extend(name.into_bytes());
                                    r.extend(attr);
                                    r.push(b'>');
                                }
                                b"local" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.local(attributes);
                                }
                                _ => {}
                            }
                        }
                    } else {
                        r.push(b'<');
                        r.extend(name.to_vec());
                        if let Some(attributes) = token.attributes() {
                            self.output_attributes(&mut r, attributes)
                        }
                        r.push(b'>');
                    }
                }
                State::ScannedEmptyElementTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::EmptyElementTag::from(token_bytes);
                    let name = token.name();
                    if name.as_bytes() == b"wd:tag" {
                        let attributes = self.parse_attibutes(&token.attributes());
                        let (name, attr) = self.custom_tag(attributes);
                        r.push(b'<');
                        r.extend(name.into_bytes());
                        r.extend(attr);
                        r.push(b' ');
                        r.push(b'/');
                        r.push(b'>');
                    } else {
                        if Self::is_wd_tag(&name) {
                            if let Some(parsed) = self.parse_wd_start_or_empty_tag(
                                name.local().as_bytes(),
                                &token.attributes(),
                            )? {
                                r.extend(parsed);
                            }
                        } else {
                            r.push(b'<');
                            r.extend(name.to_vec());
                            if let Some(attributes) = token.attributes() {
                                self.output_attributes(&mut r, attributes)
                            }
                            r.push(b' ');
                            r.push(b'/');
                            r.push(b'>');
                        }
                    }
                }
                State::ScannedEndTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::EndTag::from(token_bytes);
                    let name = token.name();
                    if name
                        .namespace_prefix()
                        .map_or(false, |v| v.as_bytes() == b"wd")
                    {
                        match name.local().as_bytes() {
                            b"local"
                            | b"result"
                            | b"record"
                            | b"collections"
                            | b"sessions"
                            | b"session_sequence_cursor" => {
                                self.state.stack().write().unwrap().pop();
                            }
                            b"session" => {
                                if let Some(ref mut session_state) = self.sessions.pop() {
                                    if session_state.commit_on_close {
                                        self.database
                                            .write()
                                            .unwrap()
                                            .commit(&mut session_state.session);
                                    } else if session_state.clear_on_close {
                                        let _ = self
                                            .database
                                            .clone()
                                            .write()
                                            .unwrap()
                                            .session_clear(&mut session_state.session);
                                    }
                                }
                            }
                            b"tag" => {
                                if let Some(name) = tag_stack.pop() {
                                    r.extend(b"</".to_vec());
                                    r.extend(name.into_bytes());
                                    r.push(b'>');
                                }
                            }
                            _ => {}
                        }
                    } else {
                        r.extend(token_bytes.to_vec());
                    }
                }
                State::ScannedCharacters(pos)
                | State::ScannedCdata(pos)
                | State::ScannedComment(pos)
                | State::ScannedDeclaration(pos) => {
                    r.extend(xml[..pos].to_vec());
                    xml = &xml[pos..];
                }
                State::ScanningCharacters => {
                    r.extend(xml.to_vec());
                    break;
                }
                _ => {}
            }
        }

        Ok(r)
    }

    #[inline(always)]
    fn custom_tag(&mut self, attributes: AttributeMap) -> (String, Vec<u8>) {
        let mut html_attr = vec![];
        let mut name = "".to_string();
        for (key, value) in attributes {
            if let Some(value) = value {
                if key.starts_with(b"wd-tag:name") {
                    name = if let Some(value) = value.as_str() {
                        value.to_owned()
                    } else {
                        value.to_string()
                    };
                } else if key.starts_with(b"wd-attr:replace") {
                    let attr = xml_util::quot_unescape(
                        if let Some(value) = value.as_str() {
                            value.to_owned()
                        } else {
                            value.to_string()
                        }
                        .as_bytes(),
                    );
                    if attr.len() > 0 {
                        html_attr.push(b' ');
                        html_attr.extend(attr.as_bytes().to_vec());
                    }
                } else {
                    html_attr.push(b' ');
                    html_attr.extend(key.to_vec());
                    html_attr.push(b'=');
                    html_attr.push(b'"');
                    html_attr.extend(
                        value
                            .as_str()
                            .map_or("", |v| v)
                            .replace("&", "&amp;")
                            .replace("<", "&lt;")
                            .replace(">", "&gt;")
                            .into_bytes(),
                    );
                    html_attr.push(b'"');
                }
            }
        }

        (name, html_attr)
    }
}
