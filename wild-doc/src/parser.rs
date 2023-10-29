mod attr;
mod collection;
mod global;
mod process;
mod record;
mod result;
mod search;
mod session;
mod update;

use std::{ops::Deref, sync::Arc};

use anyhow::Result;
use async_recursion::async_recursion;
use hashbrown::HashMap;
use parking_lot::RwLock;

use maybe_xml::{
    scanner::{Scanner, State},
    token::{
        self,
        prop::{Attributes, TagName},
    },
};
use semilattice_database_session::{Session, SessionDatabase};
use wild_doc_script::{Vars, WildDocScript, WildDocState, WildDocValue};

use crate::{script::Var, xml_util};

#[cfg(feature = "js")]
use wild_doc_script_deno::Deno;

#[cfg(feature = "py")]
use wild_doc_script_python::WdPy;

struct SessionState {
    session: Session,
    commit_on_close: bool,
    clear_on_close: bool,
}

pub struct Parser {
    database: Arc<RwLock<SessionDatabase>>,
    sessions: Vec<SessionState>,
    scripts: HashMap<String, Box<dyn WildDocScript>>,
    state: Arc<WildDocState>,
    include_stack: Vec<String>,
}

impl Parser {
    pub fn new(database: Arc<RwLock<SessionDatabase>>, state: WildDocState) -> Result<Self> {
        let state = Arc::new(state);

        let mut scripts: hashbrown::HashMap<String, Box<dyn WildDocScript>> =
            hashbrown::HashMap::new();

        scripts.insert("var".to_owned(), Box::new(Var::new(Arc::clone(&state))?));

        #[cfg(feature = "js")]
        scripts.insert("js".to_owned(), Box::new(Deno::new(Arc::clone(&state))?));

        #[cfg(feature = "py")]
        scripts.insert("py".to_owned(), Box::new(WdPy::new(Arc::clone(&state))?));

        Ok(Self {
            scripts,
            sessions: vec![],
            database,
            state,
            include_stack: vec![],
        })
    }

    pub fn state(&self) -> &Arc<WildDocState> {
        &self.state
    }

    async fn parse_wd_start_or_empty_tag(
        &mut self,
        name: &[u8],
        attributes: Option<Attributes<'_>>,
    ) -> Result<Option<Vec<u8>>> {
        match name {
            b"print" => {
                return Ok(self.vars_from_attibutes(attributes).await.get("value").map(
                    |v| match v.deref() {
                        WildDocValue::String(s) => s.to_owned().into(),
                        WildDocValue::Binary(v) => v.to_vec(),
                        _ => v.to_str().as_bytes().into(),
                    },
                ));
            }
            b"global" => {
                let vars = self.vars_from_attibutes(attributes).await;
                if let (Some(var), Some(value)) = (vars.get("var"), vars.get("value")) {
                    self.register_global(&var.to_str(), value);
                }
            }
            b"print_escape_html" => {
                return Ok(self
                    .vars_from_attibutes(attributes)
                    .await
                    .get("value")
                    .map(|v| xml_util::escape_html(&v.to_str()).into()));
            }
            b"include" => {
                let vars = self.vars_from_attibutes(attributes).await;
                return Ok(Some(self.get_include_content(vars).await?));
            }
            b"delete_collection" => {
                let vars = self.vars_from_attibutes(attributes).await;
                self.delete_collection(vars).await;
            }
            b"session_gc" => {
                let vars = self.vars_from_attibutes(attributes).await;
                self.session_gc(vars);
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

    #[async_recursion(?Send)]
    pub async fn parse<'a>(&'a mut self, xml: &'a [u8]) -> Result<Vec<u8>> {
        let mut r: Vec<u8> = Vec::new();
        let mut tag_stack = vec![];
        let mut search_map = HashMap::new();
        let mut xml: &[u8] = xml;

        let mut scanner = Scanner::new();

        while let Some(state) = scanner.scan(&xml) {
            match state {
                State::ScannedProcessingInstruction(pos) => {
                    let token_bytes = &xml[..pos];
                    let token = token::ProcessingInstruction::from(token_bytes);
                    let target = token.target();
                    if let Some(script) = self.scripts.get_mut(target.to_str()?) {
                        if let Some(i) = token.instructions() {
                            if let Err(e) = script
                                .evaluate_module(
                                    self.include_stack.last().map_or("", |v| v),
                                    i.as_bytes(),
                                )
                                .await
                            {
                                return Err(e);
                            }
                        }
                    } else {
                        r.extend(token_bytes);
                    }
                    xml = &xml[pos..];
                }
                State::ScannedStartTag(pos) => {
                    let xml_before_start = xml;
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::StartTag::from(token_bytes);
                    let name = token.name();
                    if Self::is_wd_tag(&name) {
                        if let Some(parsed) = self
                            .parse_wd_start_or_empty_tag(
                                name.local().as_bytes(),
                                token.attributes(),
                            )
                            .await?
                        {
                            r.extend(parsed);
                        } else {
                            match name.local().as_bytes() {
                                b"session" => {
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    self.session(vars);
                                }
                                b"session_sequence_cursor" => {
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    self.session_sequence(vars);
                                }
                                b"sessions" => {
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    self.sessions(vars);
                                }
                                b"re" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let parsed = self.parse(inner_xml).await?;
                                    xml = &xml[outer_end..];
                                    r.extend(self.parse(&parsed).await?);
                                }
                                b"comment" => {
                                    let (_, outer_end) = xml_util::inner(xml);
                                    xml = &xml[outer_end..];
                                }
                                b"letitgo" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.extend(inner_xml);
                                    xml = &xml[outer_end..];
                                }
                                b"update" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    self.update(inner_xml, vars).await?;
                                    xml = &xml[outer_end..];
                                }
                                b"on" => {
                                    let (_, outer_end) = xml_util::inner(xml);
                                    r.extend(&xml_before_start[..(pos + outer_end)]);
                                    xml = &xml[outer_end..];
                                }
                                b"search" => {
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    xml = self.search(xml, vars, &mut search_map).await;
                                }
                                b"result" => {
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    self.result(vars, &mut search_map).await;
                                }
                                b"record" => {
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    self.record(vars);
                                }
                                b"collections" => {
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    self.collections(vars);
                                }
                                b"case" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    r.extend(self.case(vars, inner_xml).await?);
                                    xml = &xml[outer_end..];
                                }
                                b"if" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    r.extend(self.r#if(vars, inner_xml).await?);
                                    xml = &xml[outer_end..];
                                }
                                b"for" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    r.extend(self.r#for(vars, inner_xml).await?);
                                    xml = &xml[outer_end..];
                                }
                                b"while" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.extend(self.r#while(token.attributes(), inner_xml).await?);
                                    xml = &xml[outer_end..];
                                }
                                b"tag" => {
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    let (name, attr) = self.custom_tag(vars);
                                    tag_stack.push(name.clone());
                                    r.push(b'<');
                                    r.extend(name.into_bytes());
                                    r.extend(attr);
                                    r.push(b'>');
                                }
                                b"local" => {
                                    let vars = self.vars_from_attibutes(token.attributes()).await;
                                    self.state.stack().lock().push(vars);
                                }
                                _ => {}
                            }
                        }
                    } else {
                        r.push(b'<');
                        r.extend(name.as_bytes());
                        if let Some(attributes) = token.attributes() {
                            self.output_attributes(&mut r, attributes).await;
                        }
                        r.push(b'>');
                    }
                }
                State::ScannedEmptyElementTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::EmptyElementTag::from(token_bytes);
                    let name = token.name();
                    if name.as_bytes() == b"wd:tag" {
                        let vars = self.vars_from_attibutes(token.attributes()).await;
                        let (name, attr) = self.custom_tag(vars);
                        r.push(b'<');
                        r.extend(name.into_bytes());
                        r.extend(attr);
                        r.extend(b" />");
                    } else {
                        if Self::is_wd_tag(&name) {
                            if let Some(parsed) = self
                                .parse_wd_start_or_empty_tag(
                                    name.local().as_bytes(),
                                    token.attributes(),
                                )
                                .await?
                            {
                                r.extend(parsed);
                            }
                        } else {
                            r.push(b'<');
                            r.extend(name.as_bytes());
                            if let Some(attributes) = token.attributes() {
                                self.output_attributes(&mut r, attributes).await;
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
                    let token = token::EndTag::from(token_bytes);
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
                                self.state.stack().lock().pop();
                            }
                            b"session" => {
                                if let Some(ref mut session_state) = self.sessions.pop() {
                                    if session_state.commit_on_close {
                                        self.database
                                            .write()
                                            .commit(&mut session_state.session)
                                            .await;
                                    } else if session_state.clear_on_close {
                                        let _ = self
                                            .database
                                            .write()
                                            .session_clear(&mut session_state.session);
                                    }
                                }
                            }
                            b"tag" => {
                                if let Some(name) = tag_stack.pop() {
                                    r.extend(b"</");
                                    r.extend(name.into_bytes());
                                    r.push(b'>');
                                }
                            }
                            _ => {}
                        }
                    } else {
                        r.extend(token_bytes);
                    }
                }
                State::ScannedCharacters(pos)
                | State::ScannedCdata(pos)
                | State::ScannedComment(pos)
                | State::ScannedDeclaration(pos) => {
                    r.extend(&xml[..pos]);
                    xml = &xml[pos..];
                }
                State::ScanningCharacters => {
                    r.extend(xml);
                    break;
                }
                _ => {}
            }
        }

        Ok(r)
    }

    #[inline(always)]
    fn custom_tag(&self, vars: Vars) -> (String, Vec<u8>) {
        let mut html_attr = vec![];
        let mut name = "".into();
        for (key, value) in vars.into_iter() {
            if key.starts_with("wd-tag:name") {
                name = value.to_str().into();
            } else if key.starts_with("wd-attr:replace") {
                let attr = xml_util::quot_unescape(&value.to_str());
                if attr.len() > 0 {
                    html_attr.push(b' ');
                    html_attr.extend(attr.as_bytes());
                }
            } else {
                html_attr.push(b' ');
                html_attr.extend(key.as_bytes());
                html_attr.extend(b"=\"");
                html_attr.extend(
                    value
                        .to_str()
                        .replace("&", "&amp;")
                        .replace("<", "&lt;")
                        .replace(">", "&gt;")
                        .into_bytes(),
                );
                html_attr.push(b'"');
            }
        }

        (name, html_attr)
    }
}
