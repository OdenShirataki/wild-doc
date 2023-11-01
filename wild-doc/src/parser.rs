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
    token::{
        prop::{Attributes, TagName},
        Ty,
    },
    Lexer,
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

        let mut vars_stack = vec![];
        let mut current_vars = vars;

        let mut pos_before = 0;
        let mut pos = 0;
        let lexer = unsafe { Lexer::from_slice_unchecked(xml) };

        while let Some(token) = lexer.tokenize(&mut pos) {
            match token.ty() {
                Ty::ProcessingInstruction(pi) => {
                    if let Some(i) = pi.instructions() {
                        let target = pi.target();
                        if let Some(script) =
                            self.scripts.get_mut(unsafe { target.as_str_unchecked() })
                        {
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
                        } else {
                            r.extend(i.as_bytes());
                        }
                    }
                }
                Ty::EmptyElementTag(eet) => {
                    let name = eet.name();
                    if name.as_bytes() == b"wd:tag" {
                        let vars = self
                            .vars_from_attibutes(eet.attributes(), &current_vars)
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
                                    eet.attributes(),
                                    &current_vars,
                                )
                                .await?
                            {
                                r.extend(parsed);
                            }
                        } else {
                            r.push(b'<');
                            r.extend(name.as_bytes());
                            if let Some(attributes) = eet.attributes() {
                                self.output_attributes(&mut r, attributes, &current_vars)
                                    .await;
                            }
                            r.push(b' ');
                            r.push(b'/');
                            r.push(b'>');
                        }
                    }
                }
                Ty::StartTag(st) => {
                    let name = st.name();
                    if Self::is_wd_tag(&name) {
                        if let Some(parsed) = self
                            .parse_wd_start_or_empty_tag(
                                name.local().as_bytes(),
                                st.attributes(),
                                &current_vars,
                            )
                            .await?
                        {
                            r.extend(parsed);
                        } else {
                            match name.local().as_bytes() {
                                b"session" => {
                                    let vars = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
                                        .await;
                                    self.session(vars);
                                }
                                b"session_sequence_cursor" => {
                                    let vars = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
                                        .await;
                                    let mut new_stack = current_vars.clone();
                                    vars_stack.push(current_vars);
                                    new_stack.extend(self.session_sequence(vars));
                                    current_vars = new_stack;
                                }
                                b"sessions" => {
                                    let vars = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
                                        .await;
                                    let mut new_stack = current_vars.clone();
                                    vars_stack.push(current_vars);
                                    new_stack.extend(self.sessions(vars));
                                    current_vars = new_stack;
                                }
                                b"re" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    let parsed = self
                                        .parse(&xml[begin..inner], current_vars.clone())
                                        .await?;
                                    r.extend(self.parse(&parsed, current_vars.clone()).await?);
                                }
                                b"comment" => {
                                    xml_util::to_end(&lexer, &mut pos);
                                }
                                b"letitgo" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(&xml[begin..inner]);
                                }
                                b"update" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    let vars = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
                                        .await;
                                    self.update(&xml[begin..inner], vars, &current_vars).await?;
                                }
                                b"on" => {
                                    let (_, outer) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(&xml[pos_before..outer]);
                                }
                                b"search" => {
                                    let vars = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
                                        .await;
                                    self.search(
                                        xml,
                                        &lexer,
                                        &mut pos,
                                        vars,
                                        &mut search_map,
                                        &current_vars,
                                    )
                                    .await;
                                }
                                b"result" => {
                                    let vars = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
                                        .await;
                                    let r = self.result(vars, &mut search_map).await;
                                    let mut new_stack = current_vars.clone();
                                    vars_stack.push(current_vars);
                                    new_stack.extend(r);
                                    current_vars = new_stack;
                                }
                                b"record" => {
                                    let vars = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
                                        .await;
                                    let r = self.record(vars);
                                    let mut new_stack = current_vars.clone();
                                    vars_stack.push(current_vars);
                                    new_stack.extend(r);
                                    current_vars = new_stack;
                                }
                                b"collections" => {
                                    let vars = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
                                        .await;
                                    let mut new_stack = current_vars.clone();
                                    vars_stack.push(current_vars);
                                    new_stack.extend(self.collections(vars));
                                    current_vars = new_stack;
                                }
                                b"case" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    let vars = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
                                        .await;
                                    r.extend(
                                        self.case(vars, &xml[begin..inner], &current_vars).await?,
                                    );
                                }
                                b"if" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    if let Some(value) = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
                                        .await
                                        .get("value")
                                    {
                                        if value.as_bool().map_or(false, |v| *v) {
                                            r.extend(
                                                self.parse(
                                                    &xml[begin..inner],
                                                    current_vars.clone(),
                                                )
                                                .await?,
                                            );
                                        }
                                    }
                                }
                                b"for" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    let vars = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
                                        .await;
                                    r.extend(
                                        self.r#for(vars, &xml[begin..inner], &current_vars).await?,
                                    );
                                }
                                b"while" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(
                                        self.r#while(
                                            st.attributes(),
                                            &xml[begin..inner],
                                            &current_vars,
                                        )
                                        .await?,
                                    );
                                }
                                b"tag" => {
                                    let vars = self
                                        .vars_from_attibutes(st.attributes(), &current_vars)
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
                                        .vars_from_attibutes(st.attributes(), &current_vars)
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
                        if let Some(attributes) = st.attributes() {
                            self.output_attributes(&mut r, attributes, &current_vars)
                                .await;
                        }
                        r.push(b'>');
                    }
                }
                Ty::EndTag(et) => {
                    let name = et.name();
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
                        r.extend(token.as_bytes());
                    }
                }
                Ty::Characters(_) | Ty::Cdata(_) | Ty::Comment(_) | Ty::Declaration(_) => {
                    r.extend(token.as_bytes());
                }
            }
            pos_before = pos;
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
