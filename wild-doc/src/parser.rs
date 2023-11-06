mod attr;
mod collection;
mod process;
mod record;
mod search;
mod session;
mod update;

use std::{path::Path, sync::Arc};

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
use wild_doc_script::{IncludeAdaptor, Stack, Vars, WildDocScript, WildDocValue};

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
    stack: RwLock<Stack>,
    result_options: Mutex<Vars>,
    include_stack: Mutex<Vec<String>>,
}

impl Parser {
    pub fn new(
        database: Arc<RwLock<SessionDatabase>>,
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
        cache_dir: &Path,
        input: Option<Vars>,
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
            stack: RwLock::new(Stack::new(input)),
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
    ) -> Result<Option<Vec<u8>>> {
        match name {
            b"print" => {
                return Ok(self.vars_from_attibutes(attributes).await.get("value").map(
                    |v| match v {
                        WildDocValue::String(s) => s.to_owned().into(),
                        WildDocValue::Binary(v) => v.to_vec(),
                        _ => v.to_str().as_bytes().into(),
                    },
                ));
            }
            b"result_option" => {
                let attr = self.vars_from_attibutes(attributes).await;
                if let (Some(var), Some(value)) = (attr.get("var"), attr.get("value")) {
                    self.result_options
                        .lock()
                        .insert(var.to_str().into(), value.clone());
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
                let attr = self.vars_from_attibutes(attributes).await;
                return Ok(Some(self.get_include_content(attr).await?));
            }
            b"delete_collection" => {
                let attr = self.vars_from_attibutes(attributes).await;
                self.delete_collection(attr).await;
            }
            b"session_gc" => {
                let attr = self.vars_from_attibutes(attributes).await;
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
    pub async fn parse(&self, xml: &[u8], pos: &mut usize) -> Result<Vec<u8>> {
        let mut r: Vec<u8> = Vec::new();

        let mut deps = 0;
        let mut pos_before = 0;
        let lexer = unsafe { Lexer::from_slice_unchecked(xml) };

        while let Some(token) = lexer.tokenize(pos) {
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
                                    &self.stack.read(),
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
                        let attr = self.vars_from_attibutes(eet.attributes()).await;
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
                                )
                                .await?
                            {
                                r.extend(parsed);
                            }
                        } else {
                            r.push(b'<');
                            r.extend(name.as_bytes());
                            if let Some(attributes) = eet.attributes() {
                                self.output_attributes(&mut r, attributes).await;
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
                            .parse_wd_start_or_empty_tag(name.local().as_bytes(), st.attributes())
                            .await?
                        {
                            r.extend(parsed);
                        } else {
                            deps += 1;
                            match name.local().as_bytes() {
                                b"session" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    let session = self.session(attr);

                                    if let Some(session) = session {
                                        self.sessions.write().push(session);
                                        r.extend(self.parse(xml, pos).await?);
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
                                        r.extend(self.parse(xml, pos).await?);
                                    }
                                }
                                b"session_sequence_cursor" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    let vars = self.session_sequence(attr);
                                    self.stack.write().push(vars);
                                    r.extend(self.parse(xml, pos).await?);
                                    self.stack.write().pop();
                                }
                                b"sessions" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    let vars = self.sessions(attr);
                                    self.stack.write().push(vars);
                                    r.extend(self.parse(xml, pos).await?);
                                    self.stack.write().pop();
                                }
                                b"re" => {
                                    let parsed = self.parse(xml, pos).await?;
                                    let mut new_pos = 0;
                                    r.extend(self.parse(&parsed, &mut new_pos).await?);
                                }
                                b"comment" => {
                                    xml_util::to_end(&lexer, pos);
                                }
                                b"letitgo" => {
                                    let begin = *pos;
                                    let (inner, _) = xml_util::to_end(&lexer, pos);
                                    r.extend(&xml[begin..inner]);
                                }
                                b"update" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    self.update(xml, pos, attr).await?;
                                }
                                b"on" => {
                                    let (_, outer) = xml_util::to_end(&lexer, pos);
                                    r.extend(&xml[pos_before..outer]);
                                }
                                b"search" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    r.extend(self.search(xml, pos, attr).await?);
                                }
                                b"record" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    let vars = self.record(attr);
                                    self.stack.write().push(vars);
                                    r.extend(self.parse(xml, pos).await?);
                                    self.stack.write().pop();
                                }
                                b"collections" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    let vars = self.collections(attr);
                                    self.stack.write().push(vars);
                                    r.extend(self.parse(xml, pos).await?);
                                    self.stack.write().pop();
                                }
                                b"case" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    r.extend(self.case(xml, pos, attr).await?);
                                }
                                b"if" => {
                                    let mut matched = false;
                                    if let Some(value) =
                                        self.vars_from_attibutes(st.attributes()).await.get("value")
                                    {
                                        if value.as_bool().map_or(false, |v| *v) {
                                            matched = true;
                                            r.extend(self.parse(xml, pos).await?);
                                        }
                                    }
                                    if matched == false {
                                        xml_util::to_end(&lexer, pos);
                                    }
                                }
                                b"for" => {
                                    let begin = *pos;
                                    let (inner, _) = xml_util::to_end(&lexer, pos);
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    r.extend(self.r#for(attr, &xml[begin..inner]).await?);
                                }
                                b"while" => {
                                    let begin = *pos;
                                    let (inner, _) = xml_util::to_end(&lexer, pos);
                                    r.extend(
                                        self.r#while(st.attributes(), &xml[begin..inner]).await?,
                                    );
                                }
                                b"tag" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    let (name, attr) = self.custom_tag(attr);
                                    r.push(b'<');
                                    r.extend(name.clone().into_bytes());
                                    r.extend(attr);
                                    r.push(b'>');

                                    r.extend(self.parse(xml, pos).await?);

                                    r.extend(b"</");
                                    r.extend(name.into_bytes());
                                    r.push(b'>');
                                }
                                b"var" => {
                                    let vars = self.vars_from_attibutes(st.attributes()).await;
                                    self.stack.write().push(vars);
                                    r.extend(self.parse(xml, pos).await?);
                                    self.stack.write().pop();
                                }
                                _ => {}
                            }
                        }
                    } else {
                        //println!("{}",name.to_str().unwrap());
                        r.push(b'<');
                        r.extend(name.as_bytes());
                        if let Some(attributes) = st.attributes() {
                            self.output_attributes(&mut r, attributes).await;
                        }
                        r.push(b'>');
                        r.extend(self.parse(xml, pos).await?);
                        r.extend(b"</");
                        r.extend(name.as_bytes());
                        r.push(b'>');
                    }
                }
                Ty::EndTag(_) => {
                    deps -= 1;
                    if deps <= 0 {
                        break;
                    }
                }
                _ => {
                    r.extend(token.as_bytes());
                }
            }
            pos_before = *pos;
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
