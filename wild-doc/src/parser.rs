mod attr;
mod collection;
mod process;
mod record;
mod result;
mod search;
mod session;
mod update;

use std::{ops::Deref, path::Path, sync::Arc};

use anyhow::Result;
use async_recursion::async_recursion;
use hashbrown::HashMap;
use parking_lot::{Mutex, RwLock};

use maybe_xml::{
    token::{
        prop::{Attributes, TagName},
        Ty,
    },
    Lexer,
};
use semilattice_database_session::{Session, SessionDatabase};
use wild_doc_script::{IncludeAdaptor, Vars, WildDocScript, WildDocValue};

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
    sessions: RwLock<Vec<SessionState>>,
    scripts: HashMap<String, Box<dyn WildDocScript>>,
    include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
    result_options: Mutex<Vars>,
    include_stack: Mutex<Vec<String>>,
}

impl Parser {
    pub fn new(
        database: Arc<RwLock<SessionDatabase>>,
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
        cache_dir: &Path,
    ) -> Result<Self> {
        let mut scripts: hashbrown::HashMap<String, Box<dyn WildDocScript>> =
            hashbrown::HashMap::new();

        scripts.insert(
            "var".to_owned(),
            Box::new(Var::new(
                Arc::clone(&include_adaptor),
                cache_dir.to_owned(),
            )?),
        );

        #[cfg(feature = "js")]
        scripts.insert(
            "js".to_owned(),
            Box::new(Deno::new(
                Arc::clone(&include_adaptor),
                cache_dir.to_owned(),
            )?),
        );

        #[cfg(feature = "py")]
        scripts.insert(
            "py".to_owned(),
            Box::new(WdPy::new(
                Arc::clone(&include_adaptor),
                cache_dir.to_owned(),
            )?),
        );

        Ok(Self {
            scripts,
            sessions: RwLock::new(vec![]),
            database,
            include_adaptor,
            result_options: Mutex::new(Vars::new()),
            include_stack: Mutex::new(vec![]),
        })
    }

    pub fn result_options(&self) -> &Mutex<Vars> {
        &self.result_options
    }

    async fn parse_wd_start_or_empty_tag(
        &self,
        name: &[u8],
        attributes: Option<Attributes<'_>>,
        vars: &Vars,
    ) -> Result<Option<Vec<u8>>> {
        match name {
            b"print" => {
                return Ok(self
                    .vars_from_attibutes(attributes, vars)
                    .await
                    .get("value")
                    .map(|v| match v.deref() {
                        WildDocValue::String(s) => s.to_owned().into(),
                        WildDocValue::Binary(v) => v.to_vec(),
                        _ => v.to_str().as_bytes().into(),
                    }));
            }
            b"result_option" => {
                let attr = self.vars_from_attibutes(attributes, vars).await;
                if let (Some(var), Some(value)) = (attr.get("var"), attr.get("value")) {
                    self.result_options
                        .lock()
                        .insert(var.to_str().into(), Arc::clone(value));
                }
            }
            b"print_escape_html" => {
                return Ok(self
                    .vars_from_attibutes(attributes, vars)
                    .await
                    .get("value")
                    .map(|v| xml_util::escape_html(&v.to_str()).into()));
            }
            b"include" => {
                let attr = self.vars_from_attibutes(attributes, vars).await;
                return Ok(Some(self.get_include_content(attr, vars).await?));
            }
            b"delete_collection" => {
                let attr = self.vars_from_attibutes(attributes, vars).await;
                self.delete_collection(attr).await;
            }
            b"session_gc" => {
                let attr = self.vars_from_attibutes(attributes, vars).await;
                self.session_gc(attr);
            }
            _ => {}
        }
        Ok(None)
    }

    fn is_wd_tag(name: &TagName) -> bool {
        name.namespace_prefix()
            .map_or(false, |v| v.as_bytes() == b"wd")
    }

    #[async_recursion(?Send)]
    pub async fn parse(&self, xml: &[u8], vars: Vars) -> Result<Vec<u8>> {
        let mut r: Vec<u8> = Vec::new();

        let mut search_map = HashMap::new();

        let mut pos_before = 0;
        let mut pos = 0;
        let lexer = unsafe { Lexer::from_slice_unchecked(xml) };

        while let Some(token) = lexer.tokenize(&mut pos) {
            match token.ty() {
                Ty::ProcessingInstruction(pi) => {
                    if let Some(i) = pi.instructions() {
                        let target = pi.target();
                        if let Some(script) = self.scripts.get(unsafe { target.as_str_unchecked() })
                        {
                            if let Err(e) = script
                                .evaluate_module(
                                    self.include_stack.lock().last().map_or("", |v| v),
                                    i.to_str()?,
                                    &vars,
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
                        let attr = self.vars_from_attibutes(eet.attributes(), &vars).await;
                        let (name, attr) = self.custom_tag(attr);
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
                                    &vars,
                                )
                                .await?
                            {
                                r.extend(parsed);
                            }
                        } else {
                            r.push(b'<');
                            r.extend(name.as_bytes());
                            if let Some(attributes) = eet.attributes() {
                                self.output_attributes(&mut r, attributes, &vars).await;
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
                                &vars,
                            )
                            .await?
                        {
                            r.extend(parsed);
                        } else {
                            match name.local().as_bytes() {
                                b"session" => {
                                    let attr =
                                        self.vars_from_attibutes(st.attributes(), &vars).await;
                                    let session = self.session(attr);

                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);

                                    if let Some(session) = session {
                                        self.sessions.write().push(session);
                                        r.extend(
                                            self.parse(&xml[begin..inner], vars.clone()).await?,
                                        );
                                        if let Some(ref mut session_state) =
                                            self.sessions.write().pop()
                                        {
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
                                    } else {
                                        r.extend(
                                            self.parse(&xml[begin..inner], vars.clone()).await?,
                                        );
                                    }
                                }
                                b"session_sequence_cursor" => {
                                    let attr =
                                        self.vars_from_attibutes(st.attributes(), &vars).await;
                                    let mut new_vars = vars.clone();
                                    new_vars.extend(self.session_sequence(attr));

                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(self.parse(&xml[begin..inner], new_vars).await?);
                                }
                                b"sessions" => {
                                    let attr =
                                        self.vars_from_attibutes(st.attributes(), &vars).await;
                                    let mut new_vars = vars.clone();
                                    new_vars.extend(self.sessions(attr));

                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(self.parse(&xml[begin..inner], new_vars).await?);
                                }
                                b"re" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    let parsed =
                                        self.parse(&xml[begin..inner], vars.clone()).await?;
                                    r.extend(self.parse(&parsed, vars.clone()).await?);
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
                                    let attr =
                                        self.vars_from_attibutes(st.attributes(), &vars).await;
                                    self.update(&xml[begin..inner], attr, &vars).await?;
                                }
                                b"on" => {
                                    let (_, outer) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(&xml[pos_before..outer]);
                                }
                                b"search" => {
                                    let attr =
                                        self.vars_from_attibutes(st.attributes(), &vars).await;
                                    if let Some((name, search)) =
                                        self.search(xml, &lexer, &mut pos, attr, &vars).await
                                    {
                                        search_map.insert(name, search);
                                    }
                                }
                                b"result" => {
                                    let attr =
                                        self.vars_from_attibutes(st.attributes(), &vars).await;
                                    let mut new_vars = vars.clone();
                                    new_vars.extend(self.result(attr, &mut search_map).await);

                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(self.parse(&xml[begin..inner], new_vars).await?);
                                }
                                b"record" => {
                                    let attr =
                                        self.vars_from_attibutes(st.attributes(), &vars).await;
                                    let mut new_vars = vars.clone();
                                    new_vars.extend(self.record(attr));

                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(self.parse(&xml[begin..inner], new_vars).await?);
                                }
                                b"collections" => {
                                    let attr =
                                        self.vars_from_attibutes(st.attributes(), &vars).await;
                                    let mut new_vars = vars.clone();
                                    new_vars.extend(self.collections(attr));

                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(self.parse(&xml[begin..inner], new_vars).await?);
                                }
                                b"case" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    let attr =
                                        self.vars_from_attibutes(st.attributes(), &vars).await;
                                    r.extend(self.case(attr, &xml[begin..inner], &vars).await?);
                                }
                                b"if" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    if let Some(value) = self
                                        .vars_from_attibutes(st.attributes(), &vars)
                                        .await
                                        .get("value")
                                    {
                                        if value.as_bool().map_or(false, |v| *v) {
                                            r.extend(
                                                self.parse(&xml[begin..inner], vars.clone())
                                                    .await?,
                                            );
                                        }
                                    }
                                }
                                b"for" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    let attr =
                                        self.vars_from_attibutes(st.attributes(), &vars).await;
                                    r.extend(self.r#for(attr, &xml[begin..inner], &vars).await?);
                                }
                                b"while" => {
                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(
                                        self.r#while(st.attributes(), &xml[begin..inner], &vars)
                                            .await?,
                                    );
                                }
                                b"tag" => {
                                    let attr =
                                        self.vars_from_attibutes(st.attributes(), &vars).await;
                                    let (name, attr) = self.custom_tag(attr);
                                    r.push(b'<');
                                    r.extend(name.clone().into_bytes());
                                    r.extend(attr);
                                    r.push(b'>');

                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(self.parse(&xml[begin..inner], vars.clone()).await?);

                                    r.extend(b"</");
                                    r.extend(name.into_bytes());
                                    r.push(b'>');
                                }
                                b"local" => {
                                    let mut new_vars = vars.clone();
                                    new_vars.extend(
                                        self.vars_from_attibutes(st.attributes(), &vars).await,
                                    );

                                    let begin = pos;
                                    let (inner, _) = xml_util::to_end(&lexer, &mut pos);
                                    r.extend(self.parse(&xml[begin..inner], new_vars).await?);
                                }
                                _ => {}
                            }
                        }
                    } else {
                        r.push(b'<');
                        r.extend(name.as_bytes());
                        if let Some(attributes) = st.attributes() {
                            self.output_attributes(&mut r, attributes, &vars).await;
                        }
                        r.push(b'>');
                    }
                }
                _ => {
                    r.extend(token.as_bytes());
                }
            }
            pos_before = pos;
        }

        Ok(r)
    }

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
