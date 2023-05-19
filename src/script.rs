use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::c_void,
    io,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, RwLock},
};

use deno_runtime::{
    deno_core::{self, serde_v8, v8, v8::READ_ONLY, ModuleSpecifier},
    permissions::PermissionsContainer,
    worker::{MainWorker, WorkerOptions},
};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use semilattice_database_session::{Session, SessionDatabase};

use xmlparser::{ElementEnd, Token};

mod process;

use crate::{anyhow::Result, xml_util, IncludeAdaptor};
mod result;
mod search;
mod stack;
mod update;

mod module_loader;
use module_loader::WdModuleLoader;

pub struct Script {
    database: Arc<RwLock<SessionDatabase>>,
    sessions: Vec<(Session, bool)>,
    main_module: ModuleSpecifier,
    module_loader: Rc<WdModuleLoader>,
    permissions: PermissionsContainer,
    include_stack: Vec<String>,
}
impl Script {
    pub fn new(database: Arc<RwLock<SessionDatabase>>, module_cache_dir: PathBuf) -> Self {
        Self {
            database,
            sessions: vec![],
            main_module: deno_core::resolve_url("wd://main").unwrap(),
            module_loader: WdModuleLoader::new(module_cache_dir),
            permissions: PermissionsContainer::allow_all(),
            include_stack: vec![],
        }
    }
    pub fn parse_xml_xml_parser<T: IncludeAdaptor>(
        &mut self,
        input_json: &str,
        token: &mut xmlparser::Tokenizer,
        include_adaptor: &mut T,
    ) -> Result<super::WildDocResult> {
        let mut worker = MainWorker::bootstrap_from_options(
            self.main_module.clone(),
            self.permissions.clone(),
            WorkerOptions {
                module_loader: self.module_loader.clone(),
                ..Default::default()
            },
        );
        worker.execute_script(
            "init",
            (r#"wd={
    general:{}
    ,stack:[]
    ,result_options:{}
    ,input:"#
                .to_owned()
                + (if input_json.len() > 0 {
                    input_json
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
            let scope = &mut worker.js_runtime.handle_scope();
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
                                as *mut T)
                        };
                        if let Some(contents) = include_adaptor.include(filename) {
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
                v8::String::new(scope, "script"),
                v8::String::new(scope, "get_contents"),
                func_get_contents,
            ) {
                let addr = include_adaptor as *mut T as *mut c_void;
                let v8ext_include_adaptor = v8::External::new(scope, addr);
                wd.define_own_property(
                    scope,
                    v8str_include_adaptor.into(),
                    v8ext_include_adaptor.into(),
                    READ_ONLY,
                );

                let addr = self as *mut Self as *mut c_void;
                let v8ext_script = v8::External::new(scope, addr);
                wd.define_own_property(scope, v8str_script.into(), v8ext_script.into(), READ_ONLY);

                wd.define_own_property(
                    scope,
                    v8str_get_contents.into(),
                    v8func_get_contents.into(),
                    READ_ONLY,
                );
            }
        }
        let result_body = self.parse_xml_parser(&mut worker, token, ("", "wd"), include_adaptor)?;
        let result_options = {
            let mut result_options = String::new();
            let scope = &mut worker.js_runtime.handle_scope();
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

    fn run_script(worker: &mut MainWorker, file_name: &str, src: Cow<str>) -> Result<()> {
        let src = src.to_string();
        deno_runtime::tokio_util::run_local(async {
            let script_name = "wd://script".to_owned() + file_name;
            let mod_id = worker
                .js_runtime
                .load_side_module(&ModuleSpecifier::parse(&script_name)?, Some(src.into()))
                .await?;
            worker.evaluate_module(mod_id).await?;
            worker.run_event_loop(false).await
        })
    }
    fn run_script_xml_parser(worker: &mut MainWorker, file_name: &str, src: &str) -> Result<()> {
        deno_runtime::tokio_util::run_local(async {
            let script_name = "wd://script".to_owned() + file_name;
            let mod_id = worker
                .js_runtime
                .load_side_module(
                    &ModuleSpecifier::parse(&script_name)?,
                    Some(src.to_owned().into()),
                )
                .await?;
            worker.evaluate_module(mod_id).await?;
            worker.run_event_loop(false).await
        })
    }

    pub fn parse_xml_parser<T: IncludeAdaptor>(
        &mut self,
        worker: &mut MainWorker,
        tokenizer: &mut xmlparser::Tokenizer,
        break_tag: (&str, &str),
        include_adaptor: &mut T,
    ) -> Result<Vec<u8>> {
        let mut tag_stack = vec![];
        let mut search_map = HashMap::new();
        let mut r: Vec<u8> = Vec::new();

        while let Some(Ok(token)) = tokenizer.next() {
            println!("{:?}", token);
            match token {
                Token::EmptyDtd { span, .. }
                | Token::Declaration { span, .. }
                | Token::DtdStart { span, .. }
                | Token::DtdEnd { span, .. } => {
                    r.append(&mut span.as_bytes().to_vec());
                }
                Token::ElementStart {
                    prefix,
                    local,
                    span,
                } => {
                    let prefix = prefix.as_str();
                    if prefix == "wd" {
                        let (attribytes_str, attributes) = xml_util::attributes(tokenizer);
                        let local = local.as_str();
                        println!("{}:{}", prefix, local);
                        match local {
                            "session" => {
                                self.session_xml_parser(worker, &attributes)?;
                            }
                            "print" => {
                                r.append(&mut crate::attr_parse_or_static_xml_parser(
                                    worker,
                                    &attributes,
                                    "value",
                                ));
                            }
                            "session_gc" => {
                                self.session_gc_xml_parser(worker, &attributes)?;
                            }
                            "session_sequence_cursor" => {
                                self.session_sequence_cursor_xml_parser(worker, &attributes)?;
                            }
                            "delete_collection" => {
                                self.delete_collection_xml_parser(worker, &attributes)?;
                            }
                            "include" => {
                                r.append(&mut process::get_include_content_xml_parser(
                                    self,
                                    worker,
                                    include_adaptor,
                                    &attributes,
                                )?);
                            }
                            "re" => {
                                r.append(&mut process::re(
                                    self,
                                    &xml_util::outer_xml_parser(
                                        span.as_str(),
                                        &attribytes_str,
                                        prefix,
                                        local,
                                        tokenizer,
                                    ),
                                    worker,
                                    include_adaptor,
                                )?);
                            }
                            "letitgo" => {
                                r.append(
                                    &mut xml_util::inner_xml_parser(prefix, local, tokenizer)
                                        .into_bytes(),
                                );
                            }
                            "update" => {
                                update::update_xml_parser(
                                    self,
                                    worker,
                                    tokenizer,
                                    &attributes,
                                    include_adaptor,
                                )?;
                            }
                            "search" => {
                                search::search_xml_parser(
                                    self,
                                    worker,
                                    tokenizer,
                                    &attributes,
                                    &mut search_map,
                                );
                            }
                            "result" => {
                                result::result_xml_parser(self, worker, &attributes, &search_map);
                            }
                            "collections" => {
                                self.collections_xml_parser(worker, &attributes);
                            }
                            "sessions" => {
                                self.sessions_xml_parser(worker, &attributes);
                            }
                            "stack" => {
                                if let Some(var) =
                                    attributes.get(&("".to_string(), "var".to_string()))
                                {
                                    worker.execute_script(
                                        "stack.push",
                                        ("wd.stack.push({".to_owned()
                                            + crate::quot_unescape(&var).as_str()
                                            + "});")
                                            .into(),
                                    )?;
                                }
                            }
                            "script" => {
                                if let Err(e) = Self::run_script_xml_parser(
                                    worker,
                                    if let Some(last) = self.include_stack.last() {
                                        last
                                    } else {
                                        ""
                                    },
                                    xml_util::inner_xml_parser(prefix, local, tokenizer).as_str(),
                                ) {
                                    return Err(e);
                                }
                            }
                            "case" => {
                                r.append(&mut process::case_xml_parser(
                                    self,
                                    &attributes,
                                    &xml_util::outer_xml_parser(
                                        span.as_str(),
                                        &attribytes_str,
                                        prefix,
                                        local,
                                        tokenizer,
                                    ),
                                    worker,
                                    include_adaptor,
                                )?);
                            }
                            "if" => {
                                r.append(&mut process::r#if_xml_parser(
                                    self,
                                    &attributes,
                                    &xml_util::outer_xml_parser(
                                        span.as_str(),
                                        &attribytes_str,
                                        prefix,
                                        local,
                                        tokenizer,
                                    ),
                                    worker,
                                    include_adaptor,
                                )?);
                            }
                            "for" => {
                                r.append(&mut process::r#for_xml_parser(
                                    self,
                                    &attributes,
                                    &xml_util::outer_xml_parser(
                                        span.as_str(),
                                        &attribytes_str,
                                        prefix,
                                        local,
                                        tokenizer,
                                    ),
                                    worker,
                                    include_adaptor,
                                )?);
                            }
                            "tag" => {
                                let (name, attr) = Self::custom_tag_xml_parser(&attributes, worker);
                                tag_stack.push(name.clone());
                                r.push(b'<');
                                r.append(&mut name.into_bytes());
                                r.append(&mut attr.into_bytes());
                                r.push(b'>');
                            }
                            _ => {}
                        }
                    } else {
                        r.append(&mut span.as_bytes().to_vec());
                    }
                }
                Token::Text { text } | Token::Cdata { text, .. } => {
                    r.append(&mut text.as_bytes().to_vec());
                }
                Token::ElementEnd { end, span } => {
                    if let ElementEnd::Close(prefix, local) = end {
                        let prefix = prefix.as_str();
                        let local = local.as_str();
                        if prefix == "" && local == "wd" || (prefix, local) == break_tag {
                            break;
                        }
                        if prefix == "wd" {
                            match local {
                                "stack"
                                | "result"
                                | "collections"
                                | "sessions"
                                | "session_sequence_cursor" => {
                                    let _ = worker.execute_script(
                                        "stack.pop",
                                        "wd.stack.pop();".to_owned().into(),
                                    );
                                }
                                "session" => {
                                    if let Some((ref mut session, clear_on_close)) =
                                        self.sessions.pop()
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
                                "tag" => {
                                    if let Some(name) = tag_stack.pop() {
                                        r.append(&mut b"</".to_vec());
                                        r.append(&mut name.into_bytes());
                                        r.push(b'>');
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            r.append(&mut span.to_string().into_bytes());
                        }
                    } else {
                        r.append(&mut span.to_string().into_bytes());
                    }
                }
                _ => {}
            }
        }
        Ok(r)
    }
    pub fn parse<T: IncludeAdaptor>(
        &mut self,
        worker: &mut MainWorker,
        reader: &mut Reader<&[u8]>,
        break_tag: &[u8],
        include_adaptor: &mut T,
    ) -> Result<Vec<u8>> {
        let mut tag_stack = vec![];
        let mut search_map = HashMap::new();
        let mut r = Vec::new();
        loop {
            match reader.read_event() {
                Ok(next) => match next {
                    Event::DocType(text) => {
                        r.append(&mut b"<!DOCTYPE ".to_vec());
                        r.append(&mut text.into_inner().to_vec());
                        r.push(b'>');
                    }
                    Event::Decl(decl) => {
                        r.append(&mut b"<?".to_vec());
                        r.append(&mut decl.to_vec());
                        r.append(&mut b"?>".to_vec());
                    }
                    Event::Start(ref e) => {
                        let name = e.name();
                        let name_ref = name.as_ref();
                        match name_ref {
                            b"wd:session" => {
                                self.session(worker, e)?;
                            }
                            b"wd:print" => {
                                r.append(&mut crate::attr_parse_or_static(
                                    worker,
                                    &xml_util::attr2hash_map(e),
                                    "value",
                                ));
                            }
                            b"wd:session_gc" => {
                                self.session_gc(worker, e)?;
                            }
                            b"wd:session_sequence_cursor" => {
                                self.session_sequence_cursor(worker, e)?;
                            }
                            b"wd:delete_collection" => {
                                self.delete_collection(worker, e)?;
                            }
                            b"wd:include" => {
                                r.append(&mut process::get_include_content(
                                    self,
                                    worker,
                                    include_adaptor,
                                    &xml_util::attr2hash_map(e),
                                )?);
                            }
                            b"wd:re" => {
                                r.append(&mut process::re(
                                    self,
                                    &xml_util::outer(&next, name, reader),
                                    worker,
                                    include_adaptor,
                                )?);
                            }
                            b"wd:letitgo" => {
                                r.append(
                                    &mut reader.read_text(e.name().to_owned())?.as_bytes().to_vec(),
                                );
                            }
                            b"wd:update" => {
                                update::update(self, worker, reader, e, include_adaptor)?;
                            }
                            b"wd:search" => {
                                search::search(self, worker, reader, e, &mut search_map);
                            }
                            b"wd:result" => {
                                result::result(self, worker, e, &search_map);
                            }
                            b"wd:collections" => {
                                self.collections(worker, e);
                            }
                            b"wd:sessions" => {
                                self.sessions(worker, e);
                            }
                            b"wd:stack" => {
                                if let Some(var) = e.try_get_attribute(b"var")? {
                                    worker.execute_script(
                                        "stack.push",
                                        ("wd.stack.push({".to_owned()
                                            + crate::quot_unescape(std::str::from_utf8(
                                                &var.value,
                                            )?)
                                            .as_str()
                                            + "});")
                                            .into(),
                                    )?;
                                }
                            }
                            b"wd:script" => {
                                if let Err(e) = Self::run_script(
                                    worker,
                                    if let Some(last) = self.include_stack.last() {
                                        last
                                    } else {
                                        ""
                                    },
                                    reader.read_text(name)?,
                                ) {
                                    return Err(e);
                                }
                            }
                            b"wd:case" => {
                                r.append(&mut process::case(
                                    self,
                                    &e,
                                    &xml_util::outer(&next, name, reader),
                                    worker,
                                    include_adaptor,
                                )?);
                            }
                            b"wd:if" => {
                                r.append(&mut process::r#if(
                                    self,
                                    &e,
                                    &xml_util::outer(&next, name, reader),
                                    worker,
                                    include_adaptor,
                                )?);
                            }
                            b"wd:for" => {
                                r.append(&mut process::r#for(
                                    self,
                                    &e,
                                    &xml_util::outer(&next, name, reader),
                                    worker,
                                    include_adaptor,
                                )?);
                            }
                            b"wd:tag" => {
                                let (name, attr) = Self::custom_tag(e, worker);
                                tag_stack.push(name.clone());
                                r.push(b'<');
                                r.append(&mut name.into_bytes());
                                r.append(&mut attr.into_bytes());
                                r.push(b'>');
                            }
                            _ => {
                                if !name_ref.starts_with(b"wd:") {
                                    r.push(b'<');
                                    r.append(&mut name_ref.to_vec());
                                    r.append(&mut Self::html_attr(e, worker).into_bytes());
                                    r.push(b'>');
                                }
                            }
                        }
                    }
                    Event::Empty(ref e) => {
                        let name = e.name();
                        let name = name.as_ref();
                        match name {
                            b"wd:print" => {
                                r.append(&mut crate::attr_parse_or_static(
                                    worker,
                                    &xml_util::attr2hash_map(e),
                                    "value",
                                ));
                            }
                            b"wd:session_gc" => {
                                self.session_gc(worker, e)?;
                            }
                            b"wd:delete_collection" => {
                                self.delete_collection(worker, e)?;
                            }
                            b"wd:include" => {
                                r.append(&mut process::get_include_content(
                                    self,
                                    worker,
                                    include_adaptor,
                                    &xml_util::attr2hash_map(e),
                                )?);
                            }
                            b"wd:tag" => {
                                let (name, attr) = Self::custom_tag(e, worker);
                                r.push(b'<');
                                r.append(&mut name.into_bytes());
                                r.append(&mut attr.into_bytes());
                                r.append(&mut b" />".to_vec());
                            }
                            _ => {
                                if !name.starts_with(b"wd:") {
                                    r.push(b'<');
                                    r.append(&mut name.to_vec());
                                    r.append(&mut Self::html_attr(e, worker).into_bytes());
                                    r.append(&mut b" />".to_vec());
                                }
                            }
                        }
                    }
                    Event::End(e) => {
                        let name = e.name();
                        let name = name.as_ref();
                        if name == b"wd" || name == break_tag {
                            break;
                        } else {
                            if name.starts_with(b"wd:") {
                                match name {
                                    b"wd:stack"
                                    | b"wd:result"
                                    | b"wd:collections"
                                    | b"wd:sessions"
                                    | b"wd:session_sequence_cursor" => {
                                        let _ = worker.execute_script(
                                            "stack.pop",
                                            "wd.stack.pop();".to_owned().into(),
                                        );
                                    }
                                    b"wd:session" => {
                                        if let Some((ref mut session, clear_on_close)) =
                                            self.sessions.pop()
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
                                    b"wd:tag" => {
                                        if let Some(name) = tag_stack.pop() {
                                            r.append(&mut b"</".to_vec());
                                            r.append(&mut name.into_bytes());
                                            r.push(b'>');
                                        }
                                    }
                                    _ => {}
                                }
                            } else {
                                r.append(&mut b"</".to_vec());
                                r.append(&mut name.to_vec());
                                r.push(b'>');
                            }
                        }
                    }
                    Event::CData(c) => {
                        r.append(&mut c.into_inner().to_vec());
                    }
                    Event::Text(c) => {
                        r.append(&mut c.into_inner().to_vec());
                    }
                    Event::PI(_) => {}
                    Event::Comment(_) => {}
                    Event::Eof => {
                        break;
                    }
                },
                Err(e) => {
                    eprintln!("{:?}", e);
                }
            }
        }
        Ok(r)
    }
    fn collections(&self, worker: &mut MainWorker, e: &BytesStart) {
        let attr = xml_util::attr2hash_map(&e);
        let var = crate::attr_parse_or_static_string(worker, &attr, "var");

        let scope = &mut worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);

        let obj = v8::Object::new(scope);
        if var != "" {
            if let (Ok(array), Some(v8str_var)) = (
                deno_core::serde_v8::to_v8(scope, self.database.read().unwrap().collections()),
                v8::String::new(scope, &var),
            ) {
                obj.define_own_property(scope, v8str_var.into(), array.into(), v8::READ_ONLY);
            }
        }
        stack::push(context, scope, obj);
    }
    fn collections_xml_parser(
        &self,
        worker: &mut MainWorker,
        attributes: &HashMap<(String, String), String>,
    ) {
        let var = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "var");

        let scope = &mut worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);

        let obj = v8::Object::new(scope);
        if var != "" {
            if let (Ok(array), Some(v8str_var)) = (
                deno_core::serde_v8::to_v8(scope, self.database.read().unwrap().collections()),
                v8::String::new(scope, &var),
            ) {
                obj.define_own_property(scope, v8str_var.into(), array.into(), v8::READ_ONLY);
            }
        }
        stack::push(context, scope, obj);
    }
    fn session_xml_parser(
        &mut self,
        worker: &mut MainWorker,
        attributes: &HashMap<(String, String), String>,
    ) -> io::Result<()> {
        let session_name =
            crate::attr_parse_or_static_string_xml_parser(worker, attributes, "name");

        if session_name != "" {
            let clear_on_close =
                crate::attr_parse_or_static_xml_parser(worker, attributes, "clear_on_close");

            let expire =
                crate::attr_parse_or_static_string_xml_parser(worker, attributes, "expire");
            let expire = if expire.len() > 0 {
                expire.parse::<i64>().ok()
            } else {
                None
            };
            if let Ok(mut session) = Session::new(
                &self.database.clone().read().unwrap(),
                &session_name,
                expire,
            ) {
                let cursor =
                    crate::attr_parse_or_static_string_xml_parser(worker, attributes, "cursor");
                if cursor != "" {
                    if let Ok(cursor) = cursor.parse::<usize>() {
                        session.set_sequence_cursor(cursor)
                    }
                }
                if crate::attr_parse_or_static_xml_parser(worker, attributes, "initialize")
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
    fn session(&mut self, worker: &mut MainWorker, e: &BytesStart) -> io::Result<()> {
        let attr = xml_util::attr2hash_map(&e);
        let session_name = crate::attr_parse_or_static_string(worker, &attr, "name");
        if session_name != "" {
            let clear_on_close = crate::attr_parse_or_static(worker, &attr, "clear_on_close");
            let expire = crate::attr_parse_or_static_string(worker, &attr, "expire");
            let expire = if expire.len() > 0 {
                expire.parse::<i64>().ok()
            } else {
                None
            };
            if let Ok(mut session) = Session::new(
                &self.database.clone().read().unwrap(),
                &session_name,
                expire,
            ) {
                let cursor = crate::attr_parse_or_static_string(worker, &attr, "cursor");
                if cursor != "" {
                    if let Ok(cursor) = cursor.parse::<usize>() {
                        session.set_sequence_cursor(cursor)
                    }
                }
                if crate::attr_parse_or_static(worker, &attr, "initialize") == b"true" {
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
    fn sessions(&self, worker: &mut MainWorker, e: &BytesStart) {
        let attr = xml_util::attr2hash_map(&e);
        let var = crate::attr_parse_or_static_string(worker, &attr, "var");

        let scope = &mut worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);

        let obj = v8::Object::new(scope);
        if var != "" {
            if let (Ok(sessions), Some(v8str_var)) = (
                self.database.read().unwrap().sessions(),
                v8::String::new(scope, &var),
            ) {
                if let Ok(array) = deno_core::serde_v8::to_v8(scope, sessions) {
                    obj.define_own_property(scope, v8str_var.into(), array.into(), v8::READ_ONLY);
                }
            }
        }
        stack::push(context, scope, obj);
    }
    fn sessions_xml_parser(
        &self,
        worker: &mut MainWorker,
        attributes: &HashMap<(String, String), String>,
    ) {
        let var = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "var");

        let scope = &mut worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);

        let obj = v8::Object::new(scope);
        if var != "" {
            if let (Ok(sessions), Some(v8str_var)) = (
                self.database.read().unwrap().sessions(),
                v8::String::new(scope, &var),
            ) {
                if let Ok(array) = deno_core::serde_v8::to_v8(scope, sessions) {
                    obj.define_own_property(scope, v8str_var.into(), array.into(), v8::READ_ONLY);
                }
            }
        }
        stack::push(context, scope, obj);
    }
    fn session_sequence_cursor_xml_parser(
        &mut self,
        worker: &mut MainWorker,
        attributes: &HashMap<(String, String), String>,
    ) -> io::Result<()> {
        let str_max = {
            let s = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "max");
            if s == "" {
                "wd:session_sequence_max".to_owned()
            } else {
                s
            }
        };
        let str_current = {
            let s = crate::attr_parse_or_static_string_xml_parser(worker, attributes, "current");
            if s == "" {
                "wd:session_sequence_current".to_owned()
            } else {
                s
            }
        };

        let scope = &mut worker.js_runtime.handle_scope();
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
                    obj.define_own_property(scope, v8str_max.into(), max.into(), v8::READ_ONLY);
                    obj.define_own_property(
                        scope,
                        v8str_current.into(),
                        current.into(),
                        v8::READ_ONLY,
                    );
                }
            }
        }
        stack::push(context, scope, obj);
        Ok(())
    }
    fn session_sequence_cursor(
        &mut self,
        worker: &mut MainWorker,
        e: &BytesStart,
    ) -> io::Result<()> {
        let attr = xml_util::attr2hash_map(e);
        let str_max = {
            let s = crate::attr_parse_or_static_string(worker, &attr, "max");
            if s == "" {
                "wd:session_sequence_max".to_owned()
            } else {
                s
            }
        };
        let str_current = {
            let s = crate::attr_parse_or_static_string(worker, &attr, "current");
            if s == "" {
                "wd:session_sequence_current".to_owned()
            } else {
                s
            }
        };

        let scope = &mut worker.js_runtime.handle_scope();
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
                    obj.define_own_property(scope, v8str_max.into(), max.into(), v8::READ_ONLY);
                    obj.define_own_property(
                        scope,
                        v8str_current.into(),
                        current.into(),
                        v8::READ_ONLY,
                    );
                }
            }
        }
        stack::push(context, scope, obj);
        Ok(())
    }
    fn session_gc(&mut self, worker: &mut MainWorker, e: &BytesStart) -> io::Result<()> {
        let str_expire =
            crate::attr_parse_or_static_string(worker, &xml_util::attr2hash_map(e), "expire");
        let mut expire = 60 * 60 * 24;
        if let Ok(parsed) = str_expire.parse::<i64>() {
            expire = parsed;
        }
        self.database.clone().write().unwrap().session_gc(expire)
    }
    fn session_gc_xml_parser(
        &mut self,
        worker: &mut MainWorker,
        attributes: &HashMap<(String, String), String>,
    ) -> io::Result<()> {
        let str_expire =
            crate::attr_parse_or_static_string_xml_parser(worker, attributes, "expire");
        let mut expire = 60 * 60 * 24;
        if let Ok(parsed) = str_expire.parse::<i64>() {
            expire = parsed;
        }
        self.database.clone().write().unwrap().session_gc(expire)
    }
    fn delete_collection_xml_parser(
        &mut self,
        worker: &mut MainWorker,
        attributes: &HashMap<(String, String), String>,
    ) -> Result<()> {
        let str_collection =
            crate::attr_parse_or_static_string_xml_parser(worker, attributes, "collection");
        self.database
            .clone()
            .write()
            .unwrap()
            .delete_collection(&str_collection)
    }
    fn delete_collection(&mut self, worker: &mut MainWorker, e: &BytesStart) -> Result<()> {
        let str_collection =
            crate::attr_parse_or_static_string(worker, &xml_util::attr2hash_map(e), "collection");
        self.database
            .clone()
            .write()
            .unwrap()
            .delete_collection(&str_collection)
    }
    fn html_attr(e: &BytesStart, worker: &mut MainWorker) -> String {
        let scope = &mut worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);
        let mut html_attr = "".to_string();
        for attr in e.html_attributes().with_checks(false) {
            if let Ok(attr) = attr {
                if let Ok(attr_key) = std::str::from_utf8(attr.key.as_ref()) {
                    if attr_key == "wd-attr:replace" {
                        if let Ok(value) = std::str::from_utf8(&attr.value) {
                            let attr =
                                crate::eval_result_string(scope, &crate::quot_unescape(value));
                            if attr.len() > 0 {
                                html_attr.push(' ');
                                html_attr.push_str(&attr);
                            }
                        }
                    } else {
                        let is_wd = attr_key.starts_with("wd:");
                        let attr_key = if is_wd {
                            attr_key.split_at(3).1
                        } else {
                            attr_key
                        };
                        html_attr.push(' ');
                        html_attr.push_str(attr_key);

                        if let Ok(value) = std::str::from_utf8(&attr.value) {
                            if value != "" {
                                html_attr.push_str("=\"");
                                if is_wd {
                                    html_attr.push_str(&crate::eval_result_string(scope, value));
                                } else {
                                    html_attr.push_str(
                                        &value
                                            .replace("&", "&amp;")
                                            .replace("<", "&lt;")
                                            .replace(">", "&gt;"),
                                    );
                                }
                                html_attr.push('"');
                            }
                        }
                    }
                }
            }
        }
        html_attr
    }
    fn custom_tag(e: &BytesStart, worker: &mut MainWorker) -> (String, String) {
        let scope = &mut worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);
        let mut html_attr = "".to_string();
        let mut name = "".to_string();
        for attr in e.html_attributes().with_checks(false) {
            if let Ok(attr) = attr {
                if let Ok(attr_key) = std::str::from_utf8(attr.key.as_ref()) {
                    if attr_key == "wd-tag:name" {
                        if let Ok(value) = std::str::from_utf8(&attr.value) {
                            name = crate::eval_result_string(scope, value);
                        }
                    } else {
                        if attr_key == "wd-attr:replace" {
                            if let Ok(value) = std::str::from_utf8(&attr.value) {
                                let attr =
                                    crate::eval_result_string(scope, &crate::quot_unescape(value));
                                if attr.len() > 0 {
                                    html_attr.push(' ');
                                    html_attr.push_str(&attr);
                                }
                            }
                        } else {
                            let is_wd = attr_key.starts_with("wd:");
                            let attr_key = if is_wd {
                                attr_key.split_at(3).1
                            } else {
                                attr_key
                            };
                            html_attr.push(' ');
                            html_attr.push_str(attr_key);

                            if let Ok(value) = std::str::from_utf8(&attr.value) {
                                html_attr.push_str("=\"");
                                if is_wd {
                                    html_attr.push_str(&crate::eval_result_string(scope, value));
                                } else {
                                    html_attr.push_str(
                                        &value
                                            .replace("&", "&amp;")
                                            .replace("<", "&lt;")
                                            .replace(">", "&gt;"),
                                    );
                                }
                                html_attr.push('"');
                            }
                        }
                    }
                }
            }
        }
        (name, html_attr)
    }

    fn custom_tag_xml_parser(
        attributes: &HashMap<(String, String), String>,
        worker: &mut MainWorker,
    ) -> (String, String) {
        let scope = &mut worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);
        let mut html_attr = "".to_string();
        let mut name = "".to_string();
        for ((prefix, local), value) in attributes {
            if prefix == "wd-tag" && local == "name" {
                name = crate::eval_result_string(scope, value);
            } else {
                if prefix == "wd-attr" && local == "replace" {
                    let attr = crate::eval_result_string(scope, &crate::quot_unescape(value));
                    if attr.len() > 0 {
                        html_attr.push(' ');
                        html_attr.push_str(&attr);
                    }
                } else {
                    let is_wd = prefix == "wd";
                    let attr_key = if is_wd {
                        local.to_owned()
                    } else {
                        prefix.to_owned() + ":" + local.as_str()
                    };
                    html_attr.push(' ');
                    html_attr.push_str(&attr_key);

                    html_attr.push_str("=\"");
                    if is_wd {
                        html_attr.push_str(&crate::eval_result_string(scope, value));
                    } else {
                        html_attr.push_str(
                            &value
                                .replace("&", "&amp;")
                                .replace("<", "&lt;")
                                .replace(">", "&gt;"),
                        );
                    }
                    html_attr.push('"');
                }
            }
        }
        (name, html_attr)
    }
}

fn get_wddb<'s>(scope: &mut v8::HandleScope<'s>) -> Option<&'s mut Arc<RwLock<SessionDatabase>>> {
    if let Some(script) = v8::String::new(scope, "wd.script")
        .and_then(|code| v8::Script::compile(scope, code, None))
        .and_then(|v| v.run(scope))
    {
        Some(
            &mut unsafe { &mut *(v8::Local::<v8::External>::cast(script).value() as *mut Script) }
                .database,
        )
    } else {
        None
    }
}
