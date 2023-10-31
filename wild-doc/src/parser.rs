mod attr;
mod collection;
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
    result_options: Vars,
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
            result_options: Vars::new(),
            include_stack: vec![],
        })
    }

    pub fn result_options(&self) -> &Vars {
        &self.result_options
    }

    async fn parse_wd_start_or_empty_tag(
        &mut self,
        name: &[u8],
        attributes: Option<Attributes<'_>>,
        stack: &Vars,
    ) -> Result<Option<Vec<u8>>> {
        match name {
            b"print" => {
                return Ok(self
                    .vars_from_attibutes(attributes, stack)
                    .await
                    .get("value")
                    .map(|v| match v.deref() {
                        WildDocValue::String(s) => s.to_owned().into(),
                        WildDocValue::Binary(v) => v.to_vec(),
                        _ => v.to_str().as_bytes().into(),
                    }));
            }
            b"result_option" => {
                let vars = self.vars_from_attibutes(attributes, stack).await;
                if let (Some(var), Some(value)) = (vars.get("var"), vars.get("value")) {
                    self.result_options
                        .insert(var.to_str().into(), Arc::clone(value));
                }
            }
            b"print_escape_html" => {
                return Ok(self
                    .vars_from_attibutes(attributes, stack)
                    .await
                    .get("value")
                    .map(|v| xml_util::escape_html(&v.to_str()).into()));
            }
            b"include" => {
                let vars = self.vars_from_attibutes(attributes, stack).await;
                return Ok(Some(self.get_include_content(vars, stack).await?));
            }
            b"delete_collection" => {
                let vars = self.vars_from_attibutes(attributes, stack).await;
                self.delete_collection(vars).await;
            }
            b"session_gc" => {
                let vars = self.vars_from_attibutes(attributes, stack).await;
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
    pub async fn parse<'a>(&'a mut self, xml: &'a [u8], vars: Vars) -> Result<Vec<u8>> {
        let mut r: Vec<u8> = Vec::new();
        let mut tag_stack = vec![];
        let mut search_map = HashMap::new();
        let mut xml: &[u8] = xml;

        let mut vars_stack = vec![];
        let mut current_vars = vars;

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
                                    i.to_str()?,
                                    &current_vars,
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
                                &current_vars,
                            )
                            .await?
                        {
                            r.extend(parsed);
                        } else {
                            match name.local().as_bytes() {
                                b"session" => {
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    self.session(vars);
                                }
                                b"session_sequence_cursor" => {
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    let mut new_stack = current_vars.clone();
                                    vars_stack.push(current_vars);
                                    new_stack.extend(self.session_sequence(vars));
                                    current_vars = new_stack;
                                }
                                b"sessions" => {
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    let mut new_stack = current_vars.clone();
                                    vars_stack.push(current_vars);
                                    new_stack.extend(self.sessions(vars));
                                    current_vars = new_stack;
                                }
                                b"re" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let parsed =
                                        self.parse(inner_xml, current_vars.clone()).await?;
                                    xml = &xml[outer_end..];
                                    r.extend(self.parse(&parsed, current_vars.clone()).await?);
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
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    self.update(inner_xml, vars, &current_vars).await?;
                                    xml = &xml[outer_end..];
                                }
                                b"on" => {
                                    let (_, outer_end) = xml_util::inner(xml);
                                    r.extend(&xml_before_start[..(pos + outer_end)]);
                                    xml = &xml[outer_end..];
                                }
                                b"search" => {
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    xml = self
                                        .search(xml, vars, &mut search_map, &current_vars)
                                        .await;
                                }
                                b"result" => {
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    let r = self.result(vars, &mut search_map).await;
                                    let mut new_stack = current_vars.clone();
                                    vars_stack.push(current_vars);
                                    new_stack.extend(r);
                                    current_vars = new_stack;
                                }
                                b"record" => {
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    let r = self.record(vars);
                                    let mut new_stack = current_vars.clone();
                                    vars_stack.push(current_vars);
                                    new_stack.extend(r);
                                    current_vars = new_stack;
                                }
                                b"collections" => {
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    let mut new_stack = current_vars.clone();
                                    vars_stack.push(current_vars);
                                    new_stack.extend(self.collections(vars));
                                    current_vars = new_stack;
                                }
                                b"case" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    r.extend(self.case(vars, inner_xml, &current_vars).await?);
                                    xml = &xml[outer_end..];
                                }
                                b"if" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    r.extend(self.r#if(vars, inner_xml, &current_vars).await?);
                                    xml = &xml[outer_end..];
                                }
                                b"for" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    r.extend(self.r#for(vars, inner_xml, &current_vars).await?);
                                    xml = &xml[outer_end..];
                                }
                                b"while" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.extend(
                                        self.r#while(token.attributes(), inner_xml, &current_vars)
                                            .await?,
                                    );
                                    xml = &xml[outer_end..];
                                }
                                b"tag" => {
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    let (name, attr) = self.custom_tag(vars);
                                    tag_stack.push(name.clone());
                                    r.push(b'<');
                                    r.extend(name.into_bytes());
                                    r.extend(attr);
                                    r.push(b'>');
                                }
                                b"local" => {
                                    let vars = self
                                        .vars_from_attibutes(token.attributes(), &current_vars)
                                        .await;
                                    let mut new_stack = current_vars.clone();
                                    vars_stack.push(current_vars);
                                    new_stack.extend(vars);
                                    current_vars = new_stack;
                                }
                                _ => {}
                            }
                        }
                    } else {
                        r.push(b'<');
                        r.extend(name.as_bytes());
                        if let Some(attributes) = token.attributes() {
                            self.output_attributes(&mut r, attributes, &current_vars)
                                .await;
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
                        let vars = self
                            .vars_from_attibutes(token.attributes(), &current_vars)
                            .await;
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
                                    &current_vars,
                                )
                                .await?
                            {
                                r.extend(parsed);
                            }
                        } else {
                            r.push(b'<');
                            r.extend(name.as_bytes());
                            if let Some(attributes) = token.attributes() {
                                self.output_attributes(&mut r, attributes, &current_vars)
                                    .await;
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
                                if let Some(v) = vars_stack.pop() {
                                    current_vars = v;
                                }
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
