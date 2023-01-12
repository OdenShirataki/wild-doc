use deno_runtime::{
    deno_broadcast_channel::InMemoryBroadcastChannel,
    deno_core::{self, v8, v8::READ_ONLY, ModuleSpecifier},
    deno_web::BlobStore,
    permissions::Permissions,
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
mod update;

mod module_loader;
use module_loader::WdModuleLoader;

macro_rules! located_script_name {
    () => {
        format!(
            "[deno:{}:{}:{}]",
            std::file!(),
            std::line!(),
            std::column!()
        )
    };
}

pub struct Script {
    database: Arc<RwLock<Database>>,
    sessions: Vec<(Session, bool)>,
    main_module: ModuleSpecifier,
    module_loader: Rc<WdModuleLoader>,
    bootstrap: BootstrapOptions,
    permissions: Permissions,
}
impl Script {
    pub fn new(database: Arc<RwLock<Database>>, module_cache_dir: PathBuf) -> Self {
        Self {
            database,
            sessions: vec![],
            main_module: deno_core::resolve_path("mainworker").unwrap(),
            module_loader: WdModuleLoader::new(module_cache_dir),
            bootstrap: Default::default(),
            permissions: Permissions::allow_all(),
        }
    }
    pub fn parse_xml<T: IncludeAdaptor>(
        &mut self,
        input_json: &str,
        reader: &mut Reader<&[u8]>,
        include_adaptor: &mut T,
    ) -> io::Result<super::WildDocResult> {
        let options = WorkerOptions {
            bootstrap: self.bootstrap.clone(),
            extensions: vec![],
            extensions_with_js: vec![],
            startup_snapshot: None,
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
            format_js_error_fn: None,
            source_map_getter: None,
            maybe_inspector_server: None,
            should_break_on_first_statement: false,
            should_wait_for_inspector_session: false,
            get_error_class_fn: Default::default(),
            cache_storage_dir: None,
            origin_storage_dir: None,
            blob_store: BlobStore::default(),
            broadcast_channel: InMemoryBroadcastChannel::default(),
            shared_array_buffer_store: None,
            compiled_wasm_module_store: None,
            stdio: Default::default(),
        };

        let mut worker = MainWorker::bootstrap_from_options(
            self.main_module.clone(),
            self.permissions.clone(),
            options,
        );
        let _ = worker.execute_script(
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
        );
        {
            let scope = &mut worker.js_runtime.handle_scope();
            let context = scope.get_current_context();
            let scope = &mut v8::ContextScope::new(scope, context);
            if let (Some(wd), Some(v8str_db)) = (
                v8::String::new(scope, "wd")
                    .and_then(|code| v8::Script::compile(scope, code, None))
                    .and_then(|v| v.run(scope)),
                v8::String::new(scope, "db"),
            ) {
                if let Ok(wd) = v8::Local::<v8::Object>::try_from(wd) {
                    let addr = &mut self.database as *mut Arc<RwLock<Database>> as *mut c_void;
                    let v8_ext = v8::External::new(scope, addr);
                    wd.define_own_property(scope, v8str_db.into(), v8_ext.into(), READ_ONLY);
                }
            }
        }
        let result_body = self.parse(&mut worker, reader, "wd", include_adaptor)?;
        let result_options = {
            let mut result_options = String::new();
            let scope = &mut worker.js_runtime.handle_scope();
            let context = scope.get_current_context();
            let scope = &mut v8::ContextScope::new(scope, context);
            if let Some(v) = v8::String::new(scope, "wd.result_options")
                .and_then(|code| v8::Script::compile(scope, code, None))
                .and_then(|v| v.run(scope))
            {
                if let Some(json) = v8::json::stringify(scope, v) {
                    result_options = json.to_rust_string_lossy(scope);
                }
            }
            result_options
        };
        Ok(super::WildDocResult {
            body: result_body,
            options_json: result_options,
        })
    }
    fn run_script(worker: &mut MainWorker, src: Cow<str>) {
        let src = src.to_string();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _r = runtime.block_on(async {
            let n = ModuleSpecifier::parse("wd://script")?;
            match worker.js_runtime.load_side_module(&n, Some(src)).await {
                Ok(mod_id) => {
                    worker.evaluate_module(mod_id).await?;
                    loop {
                        worker.run_event_loop(false).await?;
                        match worker.dispatch_beforeunload_event(&located_script_name!()) {
                            Ok(default_prevented) if default_prevented => {}
                            Ok(_) => break Ok(()),
                            Err(error) => break Err(error),
                        }
                    }
                }
                Err(error) => Err(error),
            }
        });
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
    pub fn parse<T: IncludeAdaptor>(
        &mut self,
        worker: &mut MainWorker,
        reader: &mut Reader<&[u8]>,
        break_tag: &str,
        include_adaptor: &mut T,
    ) -> io::Result<Vec<u8>> {
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
                                let attr = xml_util::attr2hash_map(&e);
                                let session_name =
                                    crate::attr_parse_or_static_string(worker, &attr, "name");
                                if session_name != "" {
                                    let clear_on_close = crate::attr_parse_or_static(
                                        worker,
                                        &attr,
                                        "clear_on_close",
                                    );
                                    let expire =
                                        crate::attr_parse_or_static_string(worker, &attr, "expire");
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
                                        if crate::attr_parse_or_static(worker, &attr, "initialize")
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
                            }
                            b"wd:session_gc" => {
                                self.session_gc(worker, e)?;
                            }
                            b"wd:update" => {
                                let with_commit = crate::attr_parse_or_static(
                                    worker,
                                    &xml_util::attr2hash_map(&e),
                                    "commit",
                                ) == b"1";
                                let inner_xml =
                                    self.parse(worker, reader, "wd:update", include_adaptor)?;
                                let mut inner_reader =
                                    Reader::from_str(std::str::from_utf8(&inner_xml).unwrap());
                                inner_reader.check_end_names(false);
                                let updates =
                                    update::make_update_struct(self, &mut inner_reader, worker);
                                if let Some((ref mut session, _)) = self.sessions.last_mut() {
                                    self.database
                                        .clone()
                                        .read()
                                        .unwrap()
                                        .update(session, updates)?;
                                    if with_commit {
                                        self.database.clone().write().unwrap().commit(session)?;
                                    }
                                }
                            }
                            b"wd:search" => {
                                let attr = xml_util::attr2hash_map(&e);
                                let name =
                                    crate::attr_parse_or_static_string(worker, &attr, "name");
                                let collection_name =
                                    crate::attr_parse_or_static_string(worker, &attr, "collection");
                                if name != "" && collection_name != "" {
                                    if let Some(collection_id) = self
                                        .database
                                        .clone()
                                        .read()
                                        .unwrap()
                                        .collection_id(&collection_name)
                                    {
                                        let condition =
                                            search::make_conditions(self, &attr, reader, worker);
                                        search_map
                                            .insert(name.to_owned(), (collection_id, condition));
                                    }
                                }
                            }
                            b"wd:result" => {
                                result::result(self, worker, e, &search_map);
                            }
                            b"wd:collections" => {
                                let attr = xml_util::attr2hash_map(&e);
                                let var = crate::attr_parse_or_static_string(worker, &attr, "var");

                                let scope = &mut worker.js_runtime.handle_scope();
                                let context = scope.get_current_context();
                                let scope = &mut v8::ContextScope::new(scope, context);
                                if let (Some(v8str_wd), Some(v8str_stack), Some(v8str_var)) = (
                                    v8::String::new(scope, "wd"),
                                    v8::String::new(scope, "stack"),
                                    v8::String::new(scope, &var),
                                ) {
                                    let global = context.global(scope);
                                    if let Some(wd) = global.get(scope, v8str_wd.into()) {
                                        if let Ok(wd) = v8::Local::<v8::Object>::try_from(wd) {
                                            if let Some(stack) = wd.get(scope, v8str_stack.into()) {
                                                if let Ok(stack) =
                                                    v8::Local::<v8::Array>::try_from(stack)
                                                {
                                                    let obj = v8::Object::new(scope);
                                                    if var != "" {
                                                        let collections = self
                                                            .database
                                                            .read()
                                                            .unwrap()
                                                            .collections();

                                                        let array = v8::Array::new(
                                                            scope,
                                                            collections.len() as i32,
                                                        );
                                                        for (i, collection) in
                                                            collections.iter().enumerate()
                                                        {
                                                            if let Some(v8_str) =
                                                                v8::String::new(scope, &collection)
                                                            {
                                                                array.set_index(
                                                                    scope,
                                                                    i as u32,
                                                                    v8_str.into(),
                                                                );
                                                            }
                                                        }
                                                        obj.define_own_property(
                                                            scope,
                                                            v8str_var.into(),
                                                            array.into(),
                                                            READ_ONLY,
                                                        );
                                                    }
                                                    stack.set_index(
                                                        scope,
                                                        stack.length(),
                                                        obj.into(),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
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
                                    Self::run_script(worker, src);
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
                            b"wd:include" => {
                                let attr = xml_util::attr2hash_map(e);
                                let xml = if let Some(xml) = include_adaptor.include(
                                    &crate::attr_parse_or_static_string(worker, &attr, "src"),
                                ) {
                                    Some(xml)
                                } else {
                                    let substitute = crate::attr_parse_or_static_string(
                                        worker,
                                        &attr,
                                        "substitute",
                                    );
                                    if let Some(xml) = include_adaptor.include(&substitute) {
                                        Some(xml)
                                    } else {
                                        None
                                    }
                                };
                                if let Some(xml) = xml {
                                    if xml.len() > 0 {
                                        let str_xml = "<root>".to_owned() + &xml + "</root>";
                                        let mut event_reader_inner = Reader::from_str(&str_xml);
                                        event_reader_inner.check_end_names(false);
                                        loop {
                                            match event_reader_inner.read_event() {
                                                Ok(Event::Start(e)) => {
                                                    if e.name().as_ref() == b"root" {
                                                        r.append(&mut self.parse(
                                                            worker,
                                                            &mut event_reader_inner,
                                                            "root",
                                                            include_adaptor,
                                                        )?);
                                                        break;
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
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
                        if name == b"wd" || name == break_tag.as_bytes() {
                            break;
                        } else {
                            if name.starts_with(b"wd:") {
                                if name == b"wd:stack"
                                    || name == b"wd:result"
                                    || name == b"wd:collections"
                                {
                                    let _ = worker.execute_script("stack.pop", "wd.stack.pop();");
                                } else if name == b"wd:session" {
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

    fn html_attr(e: &BytesStart, worker: &mut MainWorker) -> String {
        let scope = &mut worker.js_runtime.handle_scope();
        let context = scope.get_current_context();
        let scope = &mut v8::ContextScope::new(scope, context);
        let mut html_attr = "".to_string();
        for attr in e.attributes() {
            if let Ok(attr) = attr {
                if let Ok(attr_key) = std::str::from_utf8(attr.key.as_ref()) {
                    let is_wd = attr_key.starts_with("wd:");
                    let attr_key = if is_wd {
                        attr_key.split_at(3).1
                    } else {
                        attr_key
                    };
                    html_attr.push(' ');
                    html_attr.push_str(attr_key);
                    html_attr.push_str("=\"");

                    if let Ok(value) = std::str::from_utf8(&attr.value) {
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
                    }
                    html_attr.push('"');
                }
            }
        }
        html_attr
    }
}
