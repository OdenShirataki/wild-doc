use deno_runtime::{
    deno_broadcast_channel::InMemoryBroadcastChannel,
    deno_core::{self, error::AnyError, serde_v8, v8, v8::READ_ONLY, ModuleSpecifier},
    deno_web::BlobStore,
    fmt_errors::format_js_error,
    js::deno_isolate_init,
    permissions::PermissionsContainer,
    worker::{MainWorker, WorkerOptions},
    BootstrapOptions,
};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use semilattice_database::{Database, Session};
use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::c_void,
    io,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, RwLock},
};

mod process;

use crate::{xml_util, IncludeAdaptor};
mod result;
mod search;
mod stack;
mod update;

mod module_loader;
use module_loader::WdModuleLoader;

pub struct Script {
    database: Arc<RwLock<Database>>,
    sessions: Vec<(Session, bool)>,
    main_module: ModuleSpecifier,
    module_loader: Rc<WdModuleLoader>,
    bootstrap: BootstrapOptions,
    permissions: PermissionsContainer,
    include_stack: Vec<String>,
}
impl Script {
    pub fn new(database: Arc<RwLock<Database>>, module_cache_dir: PathBuf) -> Self {
        Self {
            database,
            sessions: vec![],
            main_module: deno_core::resolve_path("main").unwrap(),
            module_loader: WdModuleLoader::new(module_cache_dir),
            bootstrap: Default::default(),
            permissions: PermissionsContainer::allow_all(),
            include_stack: vec![],
        }
    }
    pub fn parse_xml<T: IncludeAdaptor>(
        &mut self,
        input_json: &str,
        reader: &mut Reader<&[u8]>,
        include_adaptor: &mut T,
    ) -> Result<super::WildDocResult, AnyError> {
        let options = WorkerOptions {
            bootstrap: self.bootstrap.clone(),
            extensions: vec![],
            extensions_with_js: vec![],
            startup_snapshot: Some(deno_isolate_init()),
            unsafely_ignore_certificate_errors: None,
            root_cert_store: None,
            seed: None,
            module_loader: self.module_loader.clone(),
            npm_resolver: None,
            create_web_worker_cb: Arc::new(|_| unimplemented!("web workers are not supported")),
            web_worker_preload_module_cb: Arc::new(|_| {
                unimplemented!("web workers are not supported")
            }),
            web_worker_pre_execute_module_cb: Arc::new(|_| {
                unimplemented!("web workers are not supported")
            }),
            format_js_error_fn: Some(Arc::new(format_js_error)),
            source_map_getter: None,
            maybe_inspector_server: None,
            should_break_on_first_statement: false,
            should_wait_for_inspector_session: Default::default(),
            get_error_class_fn: Default::default(),
            cache_storage_dir: None,
            origin_storage_dir: None,
            blob_store: BlobStore::default(),
            broadcast_channel: InMemoryBroadcastChannel::default(),
            shared_array_buffer_store: Default::default(),
            compiled_wasm_module_store: Default::default(),
            stdio: Default::default(),
        };

        let mut worker = MainWorker::bootstrap_from_options(
            self.main_module.clone(),
            self.permissions.clone(),
            options,
        );
        worker.execute_script(
            "init",
            &(r#"wd={
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
};"#),
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
        let result_body = self.parse(&mut worker, reader, b"wd", include_adaptor)?;
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
    fn run_script(worker: &mut MainWorker, file_name: &str, src: Cow<str>) -> Result<(), AnyError> {
        let src = src.to_string();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mod_id = worker
                    .js_runtime
                    .load_side_module(
                        &ModuleSpecifier::parse(&("wd://script".to_owned() + file_name))?,
                        Some(src),
                    )
                    .await?;
                worker.evaluate_module(mod_id).await
            })
    }

    pub fn parse<T: IncludeAdaptor>(
        &mut self,
        worker: &mut MainWorker,
        reader: &mut Reader<&[u8]>,
        break_tag: &[u8],
        include_adaptor: &mut T,
    ) -> Result<Vec<u8>, AnyError> {
        let mut search_map = HashMap::new();
        let mut r = Vec::new();
        loop {
            match reader.read_event() {
                Ok(next) => match next {
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
                            b"wd:delete_collection" => {
                                self.delete_collection(worker, e)?;
                            }
                            b"wd:run" => {
                                r.append(&mut process::run(self, e, worker, include_adaptor)?);
                            }
                            b"wd:include" => {
                                r.append(&mut process::get_include_content(
                                    self,
                                    worker,
                                    include_adaptor,
                                    &xml_util::attr2hash_map(e),
                                )?);
                            }
                            b"wd:letitgo" => {
                                if let Ok(cont) = reader.read_text(e.name().to_owned()) {
                                    r.append(&mut cont.as_bytes().to_vec());
                                }
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
                                if let Ok(Some(var)) = e.try_get_attribute(b"var") {
                                    if let Ok(var) = std::str::from_utf8(&var.value) {
                                        let code = "wd.stack.push({".to_owned() + var + "});";
                                        let _ = worker.execute_script("stack.push", &code);
                                    }
                                }
                            }
                            b"wd:script" => {
                                if let Ok(src) = reader.read_text(name) {
                                    match Self::run_script(
                                        worker,
                                        if let Some(last) = self.include_stack.last() {
                                            last
                                        } else {
                                            ""
                                        },
                                        src,
                                    ) {
                                        Err(e) => {
                                            r.append(&mut e.to_string().as_bytes().to_vec());
                                            return Ok(r);
                                        }
                                        _ => {}
                                    }
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
                            _ => {
                                if !name_ref.starts_with(b"wd:") {
                                    r.push(b'<');
                                    r.append(&mut name_ref.to_vec());
                                    r.append(&mut Self::html_attr(e, worker).as_bytes().to_vec());
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
                            b"wd:run" => {
                                r.append(&mut process::run(self, e, worker, include_adaptor)?);
                            }
                            b"wd:include" => {
                                r.append(&mut process::get_include_content(
                                    self,
                                    worker,
                                    include_adaptor,
                                    &xml_util::attr2hash_map(e),
                                )?);
                            }
                            _ => {
                                if !name.starts_with(b"wd:") {
                                    r.push(b'<');
                                    r.append(&mut name.to_vec());
                                    r.append(&mut Self::html_attr(e, worker).as_bytes().to_vec());
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
                                    b"wd:stack" | b"wd:result" | b"wd:collections"
                                    | b"wd:sessions" => {
                                        let _ =
                                            worker.execute_script("stack.pop", "wd.stack.pop();");
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
                        r.append(&mut c.unescape().expect("Error!").as_bytes().to_vec());
                    }
                    Event::Eof => {
                        break;
                    }
                    _ => {}
                },
                Err(e) => {
                    println!("{:?}", e);
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
    fn session_gc(&mut self, worker: &mut MainWorker, e: &BytesStart) -> io::Result<()> {
        let str_expire =
            crate::attr_parse_or_static_string(worker, &xml_util::attr2hash_map(e), "expire");
        let mut expire = 60 * 60 * 24;
        if let Ok(parsed) = str_expire.parse::<i64>() {
            expire = parsed;
        }
        self.database.clone().write().unwrap().session_gc(expire)
    }
    fn delete_collection(
        &mut self,
        worker: &mut MainWorker,
        e: &BytesStart,
    ) -> std::io::Result<()> {
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
        for attr in e.attributes() {
            if let Ok(attr) = attr {
                if let Ok(attr_key) = std::str::from_utf8(attr.key.as_ref()) {
                    if attr_key == "wd-attr:replace" {
                        if let Ok(value) = std::str::from_utf8(&attr.value) {
                            let attr = crate::eval_result_string(scope, value);
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
        html_attr
    }
}

fn get_wddb<'s>(scope: &mut v8::HandleScope<'s>) -> Option<&'s mut Arc<RwLock<Database>>> {
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
