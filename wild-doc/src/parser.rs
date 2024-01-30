mod attr;
mod case;
mod collection;
mod include;
mod r#loop;
mod record;
mod search;
mod session;
mod sort;
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
    Reader,
};
use semilattice_database_session::{Session, SessionDatabase};
use wild_doc_script::{IncludeAdaptor, Stack, Vars, WildDocScript, WildDocValue};

use crate::{script::Var, xml_util};

#[cfg(feature = "js")]
use wild_doc_script_deno::Deno;

#[cfg(feature = "py")]
use wild_doc_script_python::WdPy;

#[cfg(feature = "image")]
use wild_doc_script_image::WdImage;

struct SessionState {
    session: Session,
    commit_on_close: bool,
    clear_on_close: bool,
}

struct ConstStrings {
    var: Arc<String>,
    value: Arc<String>,
    collection: Arc<String>,
    src: Arc<String>,
    substitute: Arc<String>,
    r#in: Arc<String>,
    key: Arc<String>,
    r#continue: Arc<String>,
    row: Arc<String>,
    collection_id: Arc<String>,
    fields: Arc<String>,
    collection_name: Arc<String>,
    serial: Arc<String>,
    uuid: Arc<String>,
    activity: Arc<String>,
    term_begin: Arc<String>,
    term_end: Arc<String>,
    depends: Arc<String>,
    field: Arc<String>,
    last_updated: Arc<String>,
    create_collection_if_not_exists: Arc<String>,
    term: Arc<String>,
    method: Arc<String>,
    name: Arc<String>,
    relation: Arc<String>,
    commit_on_close: Arc<String>,
    clear_on_close: Arc<String>,
    expire: Arc<String>,
    cursor: Arc<String>,
    initialize: Arc<String>,
    max: Arc<String>,
    session_sequence_max: Arc<String>,
    current: Arc<String>,
    session_sequence_current: Arc<String>,
    order: Arc<String>,
    result: Arc<String>,
    without_session: Arc<String>,
    commit: Arc<String>,
    update: Arc<String>,
    commit_rows: Arc<String>,
    session_rows: Arc<String>,
    base64: Arc<String>,
    delete: Arc<String>,
    inherit_depend_if_empty: Arc<String>,
    _blank: Arc<String>,
}

impl ConstStrings {
    fn new() -> Self {
        Self {
            var: Arc::new("var".into()),
            value: Arc::new("value".into()),
            collection: Arc::new("collection".into()),
            src: Arc::new("src".into()),
            substitute: Arc::new("substitute".into()),
            r#in: Arc::new("in".into()),
            key: Arc::new("key".into()),
            r#continue: Arc::new("continue".into()),
            row: Arc::new("row".into()),
            collection_id: Arc::new("collection_id".into()),
            fields: Arc::new("fields".into()),
            collection_name: Arc::new("collection_name".into()),
            serial: Arc::new("serial".into()),
            uuid: Arc::new("uuid".into()),
            activity: Arc::new("activity".into()),
            term_begin: Arc::new("term_begin".into()),
            term_end: Arc::new("term_end".into()),
            depends: Arc::new("depends".into()),
            field: Arc::new("field".into()),
            last_updated: Arc::new("last_updated".into()),
            create_collection_if_not_exists: Arc::new("create_collection_if_not_exists".into()),
            term: Arc::new("term".into()),
            method: Arc::new("method".into()),
            name: Arc::new("name".into()),
            relation: Arc::new("relation".into()),
            commit_on_close: Arc::new("commit_on_close".into()),
            clear_on_close: Arc::new("clear_on_close".into()),
            expire: Arc::new("expire".into()),
            cursor: Arc::new("cursor".into()),
            initialize: Arc::new("initialize".into()),
            max: Arc::new("max".into()),
            session_sequence_max: Arc::new("session_sequence_max".into()),
            current: Arc::new("current".into()),
            session_sequence_current: Arc::new("session_sequence_current".into()),
            order: Arc::new("order".into()),
            result: Arc::new("result".into()),
            without_session: Arc::new("without_session".into()),
            commit: Arc::new("commit".into()),
            update: Arc::new("update".into()),
            commit_rows: Arc::new("commit_rows".into()),
            session_rows: Arc::new("session_rows".into()),
            base64: Arc::new("base64".into()),
            delete: Arc::new("delete".into()),
            inherit_depend_if_empty: Arc::new("inherit_depend_if_empty".into()),
            _blank: Arc::new("".into()),
        }
    }
}

pub struct Parser {
    database: Arc<RwLock<SessionDatabase>>,
    sessions: Vec<SessionState>,
    scripts: HashMap<String, Box<dyn WildDocScript>>,
    include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
    stack: Box<Stack>,
    result_options: Vars,
    include_stack: Vec<Arc<String>>,
    strings: ConstStrings,
}

impl Parser {
    pub fn new(
        database: Arc<RwLock<SessionDatabase>>,
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
        cache_dir: &Path,
        input: Option<Vars>,
    ) -> Result<Self> {
        let stack = Box::new(Stack::new(input));
        let mut scripts: hashbrown::HashMap<String, Box<dyn WildDocScript>> =
            hashbrown::HashMap::new();

        scripts.insert(
            "var".to_owned(),
            Box::new(Var::new(
                Arc::clone(&include_adaptor),
                cache_dir.to_owned(),
                &stack,
            )?),
        );

        #[cfg(feature = "js")]
        scripts.insert(
            "js".to_owned(),
            Box::new(Deno::new(
                Arc::clone(&include_adaptor),
                cache_dir.to_owned(),
                &stack,
            )?),
        );

        #[cfg(feature = "py")]
        scripts.insert(
            "py".to_owned(),
            Box::new(WdPy::new(
                Arc::clone(&include_adaptor),
                cache_dir.to_owned(),
                &stack,
            )?),
        );

        #[cfg(feature = "image")]
        scripts.insert(
            "image".to_owned(),
            Box::new(WdImage::new(
                Arc::clone(&include_adaptor),
                cache_dir.to_owned(),
                &stack,
            )?),
        );

        Ok(Self {
            scripts,
            sessions: vec![],
            database,
            include_adaptor,
            stack,
            result_options: Vars::new(),
            include_stack: vec![],
            strings: ConstStrings::new(),
        })
    }

    pub fn result_options(&self) -> &Vars {
        &self.result_options
    }

    async fn parse_wd_start_or_empty_tag(
        &mut self,
        name: &[u8],
        attributes: Option<Attributes<'_>>,
    ) -> Result<Option<Vec<u8>>> {
        match name {
            b"print" => {
                return Ok(self
                    .vars_from_attibutes(attributes)
                    .await
                    .get(&self.strings.value)
                    .map(|v| match v {
                        WildDocValue::String(s) => s.as_str().to_owned().into(),
                        WildDocValue::Binary(v) => v.to_vec(),
                        _ => v.as_string().as_bytes().into(),
                    }));
            }
            b"result_option" => {
                let attr = self.vars_from_attibutes(attributes).await;
                if let (Some(var), Some(value)) =
                    (attr.get(&self.strings.var), attr.get(&self.strings.value))
                {
                    self.result_options.insert(var.as_string(), value.clone());
                }
            }
            b"print_escape_html" => {
                return Ok(self
                    .vars_from_attibutes(attributes)
                    .await
                    .get(&self.strings.value)
                    .map(|v| xml_util::escape_html(&v.as_string()).into()));
            }
            b"include" => {
                let attr = self.vars_from_attibutes(attributes).await;
                return Ok(Some(self.get_include_content(attr, true).await?));
            }
            b"noparse_include" => {
                let attr = self.vars_from_attibutes(attributes).await;
                return Ok(Some(self.get_include_content(attr, false).await?));
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
    pub async fn parse(&mut self, xml: &[u8], pos: &mut usize) -> Result<Vec<u8>> {
        let mut r: Vec<u8> = Vec::new();

        let mut deps = 0;
        let mut pos_before = 0;
        let reader = Reader::from_str(unsafe { std::str::from_utf8_unchecked(xml) });

        while let Some(token) = reader.tokenize(pos) {
            match token.ty() {
                Ty::ProcessingInstruction(pi) => {
                    if let Some(i) = pi.instructions() {
                        let target = pi.target();
                        if let Some(script) = self.scripts.get_mut(target.as_str()) {
                            if let Err(e) = script
                                .evaluate_module(
                                    self.include_stack.last().map_or("", |v| v),
                                    i.as_str(),
                                    &self.stack,
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
                        r.extend(name.as_str().as_bytes().to_vec());
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
                                    if let Some(session) = self.session(attr) {
                                        self.sessions.push(session);
                                        r.extend(self.parse(xml, pos).await?);
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
                                    } else {
                                        r.extend(self.parse(xml, pos).await?);
                                    }
                                }
                                b"session_sequence_cursor" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    let vars = self.session_sequence(attr);
                                    self.stack.push(vars);
                                    r.extend(self.parse(xml, pos).await?);
                                    self.stack.pop();
                                }
                                b"sessions" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    let vars = self.sessions(attr);
                                    self.stack.push(vars);
                                    r.extend(self.parse(xml, pos).await?);
                                    self.stack.pop();
                                }
                                b"re" => {
                                    let parsed = self.parse(xml, pos).await?;
                                    let mut new_pos = 0;
                                    r.extend(self.parse(&parsed, &mut new_pos).await?);
                                }
                                b"comment" => {
                                    xml_util::to_end(xml, pos);
                                }
                                b"letitgo" => {
                                    let begin = *pos;
                                    let (inner, _) = xml_util::to_end(xml, pos);
                                    r.extend(&xml[begin..inner]);
                                }
                                b"update" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    self.update(xml, pos, attr).await?;
                                }
                                b"on" => {
                                    let (_, outer) = xml_util::to_end(xml, pos);
                                    r.extend(&xml[pos_before..outer]);
                                }
                                b"search" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    r.extend(self.search(xml, pos, attr).await?);
                                }
                                b"sort" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    r.extend(self.sort(xml, pos, attr).await?);
                                }
                                b"record" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    let vars = self.record(attr);
                                    self.stack.push(vars);
                                    r.extend(self.parse(xml, pos).await?);
                                    self.stack.pop();
                                }
                                b"collections" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    let vars = self.collections(attr);
                                    self.stack.push(vars);
                                    r.extend(self.parse(xml, pos).await?);
                                    self.stack.pop();
                                }
                                b"case" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    r.extend(self.case(xml, pos, attr).await?);
                                }
                                b"if" => {
                                    let mut matched = false;
                                    if let Some(value) = self
                                        .vars_from_attibutes(st.attributes())
                                        .await
                                        .get(&self.strings.value)
                                    {
                                        if value.as_bool().map_or(false, |v| *v) {
                                            matched = true;
                                            r.extend(self.parse(xml, pos).await?);
                                        }
                                    }
                                    if matched == false {
                                        xml_util::to_end(xml, pos);
                                    }
                                }
                                b"for" => {
                                    let begin = *pos;
                                    let (inner, _) = xml_util::to_end(xml, pos);
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    r.extend(self.r#for(attr, &xml[begin..inner]).await?);
                                }
                                b"while" => {
                                    let begin = *pos;
                                    let (inner, _) = xml_util::to_end(xml, pos);
                                    r.extend(
                                        self.r#while(st.attributes(), &xml[begin..inner]).await?,
                                    );
                                }
                                b"tag" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    let (name, attr) = self.custom_tag(attr);
                                    r.push(b'<');
                                    r.extend(name.as_bytes().to_vec());
                                    r.extend(attr);
                                    r.push(b'>');

                                    r.extend(self.parse(xml, pos).await?);

                                    r.extend(b"</");
                                    r.extend(name.as_bytes().to_vec());
                                    r.push(b'>');
                                }
                                b"var" => {
                                    let attr = self.vars_from_attibutes(st.attributes()).await;
                                    self.stack.push(attr);
                                    r.extend(self.parse(xml, pos).await?);
                                    self.stack.pop();
                                }
                                _ => {}
                            }
                        }
                    } else {
                        r.push(b'<');
                        r.extend(name.as_bytes());
                        if let Some(attributes) = st.attributes() {
                            self.output_attributes(&mut r, attributes).await;
                        }
                        r.push(b'>');
                        match name.as_bytes() {
                            b"input" | b"br" | b"hr" => {}
                            _ => {
                                r.extend(self.parse(xml, pos).await?);
                                r.extend(b"</");
                                r.extend(name.as_bytes());
                                r.push(b'>');
                            }
                        }
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

    fn custom_tag(&self, vars: Vars) -> (Arc<String>, Vec<u8>) {
        let mut html_attr = vec![];
        let mut name = Arc::new("".into());
        for (key, value) in vars.into_iter() {
            if key.starts_with("wd-tag:name") {
                name = value.as_string();
            } else if key.starts_with("wd-attr:replace") {
                let attr = xml_util::quot_unescape(&value.as_string());
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
                        .as_string()
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
