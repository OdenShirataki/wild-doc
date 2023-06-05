use std::{
    collections::HashMap,
    ffi::c_void,
    io::{self},
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use deno_runtime::{
    deno_core::{
        self, serde_v8,
        v8::{self, PropertyAttribute},
        ModuleSpecifier,
    },
    permissions::PermissionsContainer,
    worker::{MainWorker, WorkerOptions},
};
use maybe_xml::{
    scanner::{Scanner, State},
    token::{
        self,
        prop::{Attributes, TagName},
    },
};
use semilattice_database_session::{Session, SessionDatabase};

mod process;

use crate::{anyhow::Result, xml_util, IncludeAdaptor};
mod result;
mod search;
mod stack;
mod update;

mod module_loader;
use module_loader::WdModuleLoader;

pub struct Script<T: IncludeAdaptor> {
    database: Arc<RwLock<SessionDatabase>>,
    sessions: Vec<(Session, bool)>,
    worker: MainWorker,
    include_adaptor: Arc<Mutex<T>>,
    include_stack: Vec<String>,
}
impl<T: IncludeAdaptor> Script<T> {
    pub fn new(
        database: Arc<RwLock<SessionDatabase>>,
        include_adaptor: Arc<Mutex<T>>,
        module_cache_dir: PathBuf,
    ) -> Self {
        Self {
            database,
            sessions: vec![],
            worker: MainWorker::bootstrap_from_options(
                deno_core::resolve_url("wd://main").unwrap(),
                PermissionsContainer::allow_all(),
                WorkerOptions {
                    module_loader: WdModuleLoader::new(module_cache_dir),
                    ..Default::default()
                },
            ),
            include_adaptor,
            include_stack: vec![],
        }
    }

    pub fn parse_xml(&mut self, input_json: &[u8], xml: &[u8]) -> Result<super::WildDocResult> {
        self.worker.execute_script(
            "init",
            (r#"wd={
    general:{}
    ,stack:[]
    ,result_options:{}
    ,input:"#
                .to_owned()
                + (if input_json.len() > 0 {
                    std::str::from_utf8(input_json)?
                } else {
                    "{}"
                })
                + r#"
};
wd.v=key=>{
    for(let i=wd.stack.length-1;i>=0;i--){
        if(wd.stack[i][key]!==void 0){
            return wd.stack[i][key];
        }
    }
};"#)
                .into(),
        )?;
        {
            let scope = &mut self.worker.js_runtime.handle_scope();
            let context = scope.get_current_context();
            let scope = &mut v8::ContextScope::new(scope, context);

            let func_get_contents = v8::Function::new(
                scope,
                |scope: &mut v8::HandleScope,
                 args: v8::FunctionCallbackArguments,
                 mut retval: v8::ReturnValue| {
                    if let Some(include_adaptor) = v8::String::new(scope, "wd.include_adaptor")
                        .and_then(|code| v8::Script::compile(scope, code, None))
                        .and_then(|v| v.run(scope))
                    {
                        let filename = args
                            .get(0)
                            .to_string(scope)
                            .unwrap()
                            .to_rust_string_lossy(scope);
                        let include_adaptor = unsafe {
                            &mut *(v8::Local::<v8::External>::cast(include_adaptor).value()
                                as *mut Arc<RwLock<T>>)
                        };
                        if let Some(contents) = include_adaptor.write().unwrap().include(filename) {
                            if let Ok(r) = serde_v8::to_v8(scope, contents) {
                                retval.set(r.into());
                            }
                        }
                    }
                },
            );
            if let (
                Some(wd),
                Some(v8str_include_adaptor),
                Some(v8str_script),
                Some(v8str_get_contents),
                Some(v8func_get_contents),
            ) = (
                v8::String::new(scope, "wd")
                    .and_then(|code| v8::Script::compile(scope, code, None))
                    .and_then(|v| v.run(scope))
                    .and_then(|v| v8::Local::<v8::Object>::try_from(v).ok()),
                v8::String::new(scope, "include_adaptor"),
                v8::String::new(scope, "database"),
                v8::String::new(scope, "get_contents"),
                func_get_contents,
            ) {
                let v8ext_include_adaptor = v8::External::new(
                    scope,
                    &self.include_adaptor as *const Arc<Mutex<T>> as *mut c_void,
                );
                wd.define_own_property(
                    scope,
                    v8str_include_adaptor.into(),
                    v8ext_include_adaptor.into(),
                    PropertyAttribute::READ_ONLY,
                );

                let v8ext_script = v8::External::new(
                    scope,
                    &self.database as *const Arc<RwLock<SessionDatabase>> as *mut c_void,
                );
                wd.define_own_property(
                    scope,
                    v8str_script.into(),
                    v8ext_script.into(),
                    PropertyAttribute::READ_ONLY,
                );

                wd.define_own_property(
                    scope,
                    v8str_get_contents.into(),
                    v8func_get_contents.into(),
                    PropertyAttribute::READ_ONLY,
                );
            }
        }

        let result_body = self.parse(xml)?;
        let result_options = {
            let mut result_options = String::new();
            let scope = &mut self.worker.js_runtime.handle_scope();
            let context = scope.get_current_context();
            let scope = &mut v8::ContextScope::new(scope, context);
            if let Some(json) = v8::String::new(scope, "wd.result_options")
                .and_then(|code| v8::Script::compile(scope, code, None))
                .and_then(|v| v.run(scope))
                .and_then(|v| v8::json::stringify(scope, v))
            {
                result_options = json.to_rust_string_lossy(scope);
            }
            result_options
        };
        Ok(super::WildDocResult {
            body: result_body,
            options_json: result_options,
        })
    }
    fn run_script(worker: &mut MainWorker, file_name: &str, src: &[u8]) -> Result<()> {
        deno_runtime::tokio_util::create_basic_runtime().block_on(async {
            let script_name = "wd://script".to_owned() + file_name;
            let mod_id = worker
                .js_runtime
                .load_side_module(
                    &ModuleSpecifier::parse(&script_name)?,
                    Some(String::from_utf8(src.to_vec())?.into()),
                )
                .await?;
            worker.evaluate_module(mod_id).await?;
            worker.run_event_loop(false).await
        })
    }

    fn parse_wd_start_or_empty_tag(
        &mut self,
        name: &[u8],
        attributes: &Option<Attributes>,
    ) -> Result<Option<Vec<u8>>> {
        match name {
            b"print" => {
                return Ok(Some(crate::attr_parse_or_static(
                    &mut self.worker,
                    &crate::attr2map(attributes),
                    b"value",
                )));
            }
            b"include" => {
                return Ok(Some(
                    self.get_include_content(&crate::attr2map(&attributes))?,
                ));
            }
            b"delete_collection" => {
                self.delete_collection(&crate::attr2map(attributes))?;
            }
            b"session_gc" => {
                self.session_gc(&crate::attr2map(attributes))?;
            }
            _ => {}
        }
        Ok(None)
    }
    fn is_wd_tag(name: &TagName) -> bool {
        if let Some(prefix) = name.namespace_prefix() {
            prefix.as_bytes() == b"wd"
        } else {
            false
        }
    }
    fn output_attrbutes(&mut self, r: &mut Vec<u8>, attributes: &Attributes) {
        for attribute in attributes.iter() {
            r.push(b' ');
            let name = attribute.name();
            if let Some(prefix) = name.namespace_prefix() {
                match prefix.as_bytes() {
                    b"wd" => {
                        r.append(&mut name.local().to_vec());
                        if let Some(value) = attribute.value() {
                            r.push(b'=');
                            r.push(b'"');
                            r.append(
                                &mut crate::eval_result_string(
                                    &mut self.worker.js_runtime.handle_scope(),
                                    crate::quot_unescape(value.as_bytes()).as_ref(),
                                )
                                .as_bytes()
                                .to_vec(),
                            );
                            r.push(b'"');
                        }
                    }
                    b"wd-attr" => {
                        if name.local().as_bytes() == b"replace" {
                            if let Some(value) = attribute.value() {
                                r.append(
                                    &mut crate::eval_result_string(
                                        &mut self.worker.js_runtime.handle_scope(),
                                        crate::quot_unescape(value.as_bytes()).as_ref(),
                                    )
                                    .as_bytes()
                                    .to_vec(),
                                );
                            }
                        }
                    }
                    _ => {
                        r.append(&mut attribute.to_vec());
                    }
                }
            } else {
                r.append(&mut attribute.to_vec());
            }
        }
    }

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
                    if token.target().to_str()? == "typescript" {
                        if let Some(i) = token.instructions() {
                            if let Err(e) = Self::run_script(
                                &mut self.worker,
                                if let Some(last) = self.include_stack.last() {
                                    last
                                } else {
                                    ""
                                },
                                i.as_bytes(),
                            ) {
                                return Err(e);
                            }
                        }
                    } else {
                        r.append(&mut token_bytes.to_vec());
                    }
                    xml = &xml[pos..];
                }
                State::ScannedStartTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::StartTag::from(token_bytes);
                    let attributes = token.attributes();
                    let name = token.name();
                    if Self::is_wd_tag(&name) {
                        if let Some(mut parsed) =
                            self.parse_wd_start_or_empty_tag(name.local().as_bytes(), &attributes)?
                        {
                            r.append(&mut parsed);
                        } else {
                            match name.local().as_bytes() {
                                b"session" => {
                                    self.session(&crate::attr2map(&attributes))?;
                                }
                                b"session_sequence_cursor" => {
                                    self.session_sequence(&crate::attr2map(&attributes))?;
                                }
                                b"sessions" => {
                                    self.sessions(&crate::attr2map(&attributes));
                                }
                                b"re" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let parsed = self.parse(inner_xml)?;
                                    xml = &xml[outer_end..];
                                    r.append(&mut self.parse(&parsed)?);
                                }
                                b"letitgo" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(&mut inner_xml.to_vec());
                                    xml = &xml[outer_end..];
                                }
                                b"update" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    self.update(inner_xml, &crate::attr2map(&attributes))?;
                                    xml = &xml[outer_end..];
                                }
                                b"search" => {
                                    xml = self.search(
                                        xml,
                                        &crate::attr2map(&attributes),
                                        &mut search_map,
                                    );
                                }
                                b"result" => {
                                    self.result(&crate::attr2map(&attributes), &search_map);
                                }
                                b"collections" => {
                                    self.collections(&crate::attr2map(&attributes));
                                }
                                b"stack" => {
                                    let attributes = crate::attr2map(&attributes);
                                    if let Some((None, Some(var))) =
                                        attributes.get(b"var".as_slice())
                                    {
                                        self.worker.execute_script(
                                            "stack.push",
                                            ("wd.stack.push({".to_owned()
                                                + crate::quot_unescape(var).as_str()
                                                + "});")
                                                .into(),
                                        )?;
                                    }
                                }
                                b"case" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(
                                        &mut self.case(&crate::attr2map(&attributes), inner_xml)?,
                                    );
                                    xml = &xml[outer_end..];
                                }
                                b"if" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(
                                        &mut self.r#if(&crate::attr2map(&attributes), inner_xml)?,
                                    );
                                    xml = &xml[outer_end..];
                                }
                                b"for" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(
                                        &mut self
                                            .r#for(&crate::attr2map(&attributes), inner_xml)?,
                                    );
                                    xml = &xml[outer_end..];
                                }
                                b"tag" => {
                                    let mut r: Vec<u8> = Vec::new();
                                    let (name, mut attr) = Self::custom_tag(
                                        &crate::attr2map(&attributes),
                                        &mut self.worker,
                                    );
                                    tag_stack.push(name.clone());
                                    r.push(b'<');
                                    r.append(&mut name.into_bytes());
                                    r.append(&mut attr);
                                    r.push(b'>');
                                    return Ok(r);
                                }
                                _ => {}
                            }
                        }
                    } else {
                        r.push(b'<');
                        r.append(&mut name.to_vec());
                        if let Some(ref attributes) = attributes {
                            self.output_attrbutes(&mut r, attributes)
                        }
                        r.push(b'>');
                    }
                }
                State::ScannedEmptyElementTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::EmptyElementTag::from(token_bytes);
                    let attributes = token.attributes();
                    let name = token.name();
                    if name.as_bytes() == b"wd:tag" {
                        let (name, mut attr) =
                            Self::custom_tag(&crate::attr2map(&attributes), &mut self.worker);
                        r.push(b'<');
                        r.append(&mut name.into_bytes());
                        r.append(&mut attr);
                        r.push(b' ');
                        r.push(b'/');
                        r.push(b'>');
                    } else {
                        if Self::is_wd_tag(&name) {
                            if let Some(mut parsed) = self
                                .parse_wd_start_or_empty_tag(name.local().as_bytes(), &attributes)?
                            {
                                r.append(&mut parsed);
                            }
                        } else {
                            r.push(b'<');
                            r.append(&mut name.to_vec());
                            if let Some(ref attributes) = attributes {
                                self.output_attrbutes(&mut r, attributes)
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
                    if if let Some(prefix) = name.namespace_prefix() {
                        prefix.as_bytes() == b"wd"
                    } else {
                        false
                    } {
                        match name.local().as_bytes() {
                            b"stack"
                            | b"result"
                            | b"collections"
                            | b"sessions"
                            | b"session_sequence_cursor" => {
                                let _ = self.worker.execute_script(
                                    "stack.pop",
                                    "wd.stack.pop();".to_owned().into(),
                                );
                            }
                            b"session" => {
                                if let Some((ref mut session, clear_on_close)) = self.sessions.pop()
                                {
                                    if clear_on_close {
                                        let _ = self
                                            .database
                                            .clone()
                                            .write()
                                            .unwrap()
                                            .session_clear(session);
                                    }
                                }
                            }
                            b"tag" => {
                                if let Some(name) = tag_stack.pop() {
                                    r.append(&mut b"</".to_vec());
                                    r.append(&mut name.into_bytes());
                                    r.push(b'>');
                                }
                            }
                            _ => {}
                        }
                    } else {
                        r.append(&mut token_bytes.to_vec());
                    }
                }
                State::ScannedCharacters(pos)
                | State::ScannedCdata(pos)
                | State::ScannedComment(pos)
                | State::ScannedDeclaration(pos) => {
                    r.append(&mut xml[..pos].to_vec());
                    xml = &xml[pos..];
                }
                State::ScanningCharacters => {
                    r.append(&mut xml.to_vec());
                    break;
                }
                _ => {}
            }
        }

        Ok(r)
    }

    fn collections(&mut self, attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>) {
        let var = crate::attr_parse_or_static_string(&mut self.worker, attributes, b"var");

        let scope = &mut self.worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);

        let obj = v8::Object::new(scope);
        if var != "" {
            if let (Ok(array), Some(v8str_var)) = (
                deno_core::serde_v8::to_v8(scope, self.database.read().unwrap().collections()),
                v8::String::new(scope, &var),
            ) {
                obj.define_own_property(
                    scope,
                    v8str_var.into(),
                    array.into(),
                    PropertyAttribute::READ_ONLY,
                );
            }
        }
        stack::push(context, scope, obj);
    }
    fn session(
        &mut self,
        attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    ) -> io::Result<()> {
        let session_name =
            crate::attr_parse_or_static_string(&mut self.worker, &attributes, b"name");

        if session_name != "" {
            let clear_on_close =
                crate::attr_parse_or_static(&mut self.worker, &attributes, b"clear_on_close");

            let expire =
                crate::attr_parse_or_static_string(&mut self.worker, &attributes, b"expire");
            let expire = if expire.len() > 0 {
                expire.parse::<i64>().ok()
            } else {
                None
            };
            if let Ok(mut session) =
                Session::new(&self.database.read().unwrap(), &session_name, expire)
            {
                let cursor =
                    crate::attr_parse_or_static_string(&mut self.worker, &attributes, b"cursor");
                if cursor != "" {
                    if let Ok(cursor) = cursor.parse::<usize>() {
                        session.set_sequence_cursor(cursor)
                    }
                }
                if crate::attr_parse_or_static(&mut self.worker, &attributes, b"initialize")
                    == b"true"
                {
                    self.database
                        .clone()
                        .read()
                        .unwrap()
                        .session_restart(&mut session, expire)?;
                }
                self.sessions.push((session, clear_on_close == b"true"));
            }
        }
        Ok(())
    }
    fn sessions(&mut self, attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>) {
        let var = crate::attr_parse_or_static_string(&mut self.worker, attributes, b"var");

        let scope = &mut self.worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);

        let obj = v8::Object::new(scope);
        if var != "" {
            if let (Ok(sessions), Some(v8str_var)) = (
                self.database.read().unwrap().sessions(),
                v8::String::new(scope, &var),
            ) {
                if let Ok(array) = deno_core::serde_v8::to_v8(scope, sessions) {
                    obj.define_own_property(
                        scope,
                        v8str_var.into(),
                        array.into(),
                        PropertyAttribute::READ_ONLY,
                    );
                }
            }
        }
        stack::push(context, scope, obj);
    }
    fn session_sequence(
        &mut self,
        attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    ) -> io::Result<()> {
        let str_max = {
            let s = crate::attr_parse_or_static_string(&mut self.worker, attributes, b"max");
            if s == "" {
                "wd:session_sequence_max".to_owned()
            } else {
                s
            }
        };
        let str_current = {
            let s = crate::attr_parse_or_static_string(&mut self.worker, attributes, b"current");
            if s == "" {
                "wd:session_sequence_current".to_owned()
            } else {
                s
            }
        };

        let scope = &mut self.worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);

        let obj = v8::Object::new(scope);
        if let Some((session, _)) = self.sessions.last() {
            if let Some(cursor) = session.sequence_cursor() {
                if let (Some(v8str_max), Some(v8str_current)) = (
                    v8::String::new(scope, &str_max),
                    v8::String::new(scope, &str_current),
                ) {
                    let max = v8::Integer::new(scope, cursor.max as i32);
                    let current = v8::Integer::new(scope, cursor.current as i32);
                    obj.define_own_property(
                        scope,
                        v8str_max.into(),
                        max.into(),
                        PropertyAttribute::READ_ONLY,
                    );
                    obj.define_own_property(
                        scope,
                        v8str_current.into(),
                        current.into(),
                        PropertyAttribute::READ_ONLY,
                    );
                }
            }
        }
        stack::push(context, scope, obj);
        Ok(())
    }
    fn session_gc(
        &mut self,
        attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    ) -> io::Result<()> {
        let str_expire =
            crate::attr_parse_or_static_string(&mut self.worker, attributes, b"expire");
        let mut expire = 60 * 60 * 24;
        if let Ok(parsed) = str_expire.parse::<i64>() {
            expire = parsed;
        }
        self.database.write().unwrap().session_gc(expire)
    }
    fn delete_collection(
        &mut self,
        attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    ) -> Result<()> {
        let str_collection =
            crate::attr_parse_or_static_string(&mut self.worker, attributes, b"collection");
        self.database
            .clone()
            .write()
            .unwrap()
            .delete_collection(&str_collection)
    }

    fn custom_tag(
        attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
        worker: &mut MainWorker,
    ) -> (String, Vec<u8>) {
        let scope = &mut worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);
        let mut html_attr = vec![];
        let mut name = "".to_string();
        for (local, (prefix, value)) in attributes {
            if let Some(value) = value {
                let prefix = if let Some(prefix) = prefix {
                    prefix.to_vec()
                } else {
                    b"".to_vec()
                };
                if prefix == b"wd-tag" && local == b"name" {
                    name = crate::eval_result_string(scope, value);
                } else {
                    if prefix == b"wd-attr" && local == b"replace" {
                        let attr = crate::eval_result_string(
                            scope,
                            crate::quot_unescape(value).as_bytes(),
                        );
                        if attr.len() > 0 {
                            html_attr.push(b' ');
                            html_attr.append(&mut attr.into_bytes());
                        }
                    } else {
                        let is_wd = prefix == b"wd";
                        let mut attr_key = if is_wd {
                            local.to_vec()
                        } else {
                            let mut prefix = prefix.to_vec();
                            prefix.push(b':');
                            prefix.append(&mut local.to_vec());
                            prefix
                        };
                        html_attr.push(b' ');
                        html_attr.append(&mut attr_key);
                        html_attr.push(b'=');
                        html_attr.push(b'"');
                        if is_wd {
                            html_attr
                                .append(&mut crate::eval_result_string(scope, value).into_bytes());
                        } else {
                            html_attr.append(
                                &mut unsafe { std::str::from_utf8_unchecked(value) }
                                    .replace("&", "&amp;")
                                    .replace("<", "&lt;")
                                    .replace(">", "&gt;")
                                    .into_bytes(),
                            );
                        }
                        html_attr.push(b'"');
                    }
                }
            }
        }
        (name, html_attr)
    }
}

fn get_wddb<'s>(scope: &mut v8::HandleScope<'s>) -> Option<&'s Arc<RwLock<SessionDatabase>>> {
    if let Some(database) = v8::String::new(scope, "wd.database")
        .and_then(|code| v8::Script::compile(scope, code, None))
        .and_then(|v| v.run(scope))
    {
        Some(unsafe {
            &*(v8::Local::<v8::External>::cast(database).value()
                as *const Arc<RwLock<SessionDatabase>>)
        })
    } else {
        None
    }
}
