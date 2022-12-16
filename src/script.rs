use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use deno_runtime::deno_napi::v8::READ_ONLY;
use deno_runtime::{
    deno_broadcast_channel::InMemoryBroadcastChannel,
    deno_core::{self, error::AnyError, v8, ModuleSpecifier},
    deno_web::BlobStore,
    ops,
    permissions::Permissions,
    worker::{MainWorker, WorkerOptions},
    BootstrapOptions,
};
use quick_xml::events::{BytesStart, Event};

use quick_xml::Reader;
use semilattice_database::{Database, Session};

mod process;

use crate::{xml_util, IncludeAdaptor};
mod result;
mod search;
mod update;

mod module_loader;
use module_loader::WdModuleLoader;

fn get_error_class_name(e: &AnyError) -> &'static str {
    deno_runtime::errors::get_error_class_name(e).unwrap_or("Error")
}

pub struct Script {
    database: Arc<RwLock<Database>>,
    sessions: Vec<Session>,
    main_module: ModuleSpecifier,
    module_loader: Rc<WdModuleLoader>,
    bootstrap: BootstrapOptions,
    permissions: Permissions,
    create_web_worker_cb: Arc<ops::worker_host::CreateWebWorkerCb>,
    web_worker_event_cb: Arc<ops::worker_host::WorkerEventCb>,
}
impl Script {
    pub fn new(database: Arc<RwLock<Database>>) -> Self {
        Self {
            database,
            sessions: vec![],
            main_module: deno_core::resolve_path("mainworker").unwrap(),
            module_loader: WdModuleLoader::new(),
            bootstrap: BootstrapOptions {
                args: vec![],
                cpu_count: 1,
                debug_flag: false,
                enable_testing_features: false,
                locale: v8::icu::get_language_tag(),
                location: None,
                no_color: false,
                is_tty: false,
                runtime_version: "x".to_string(),
                ts_version: "x".to_string(),
                unstable: false,
                user_agent: "hello_runtime".to_string(),
                inspect: false,
            },
            permissions: Permissions::allow_all(),
            create_web_worker_cb: Arc::new(|_| {
                todo!("Web workers are not supported in the example");
            }),
            web_worker_event_cb: Arc::new(|_| {
                todo!("Web workers are not supported in the example");
            }),
        }
    }
    pub fn parse_xml<T: IncludeAdaptor>(
        &mut self,
        input_json: &str,
        reader: &mut Reader<&[u8]>,
        include_adaptor: &mut T,
    ) -> Result<super::WildDocResult, std::io::Error> {
        let options = WorkerOptions {
            bootstrap: self.bootstrap.clone(),
            extensions: vec![],
            startup_snapshot: None,
            unsafely_ignore_certificate_errors: None,
            root_cert_store: None,
            seed: None,
            module_loader: self.module_loader.clone(),
            npm_resolver: None,
            create_web_worker_cb: self.create_web_worker_cb.clone(),
            web_worker_preload_module_cb: self.web_worker_event_cb.clone(),
            web_worker_pre_execute_module_cb: self.web_worker_event_cb.clone(),
            format_js_error_fn: None,
            source_map_getter: None,
            maybe_inspector_server: None,
            should_break_on_first_statement: false,
            should_wait_for_inspector_session: false,
            get_error_class_fn: Some(&get_error_class_name),
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
        deno_core::futures::executor::block_on(async {
            let n = ModuleSpecifier::parse("wd://script").unwrap();
            if let Ok(mod_id) = worker.js_runtime.load_side_module(&n, Some(src)).await {
                let result = worker.js_runtime.mod_evaluate(mod_id);
                let _ = worker.run_event_loop(false).await;
                let _ = result.await;
            }
        });
    }
    pub fn parse<T: IncludeAdaptor>(
        &mut self,
        worker: &mut MainWorker,
        reader: &mut Reader<&[u8]>,
        break_tag: &str,
        include_adaptor: &mut T,
    ) -> Result<Vec<u8>, std::io::Error> {
        let mut search_map = HashMap::new();
        let mut r = Vec::new();
        loop {
            if let Ok(next) = reader.read_event() {
                match next {
                    Event::Start(ref e) => {
                        let name = e.name();
                        let name_ref = name.as_ref();
                        match name_ref {
                            b"wd:session" => {
                                let session_name = crate::attr_parse_or_static(
                                    worker,
                                    &xml_util::attr2hash_map(&e),
                                    "name",
                                );
                                if let Ok(mut session) = Session::new(
                                    &self.database.clone().read().unwrap(),
                                    &session_name,
                                ) {
                                    if session_name != "" {
                                        if let Ok(Some(value)) = e.try_get_attribute(b"initialize")
                                        {
                                            if value.value.to_vec() == b"true" {
                                                self.database
                                                    .clone()
                                                    .read()
                                                    .unwrap()
                                                    .session_restart(&mut session)?;
                                            }
                                        }
                                    }
                                    self.sessions.push(session);
                                } else {
                                    xml_util::outer(&next, reader);
                                }
                            }
                            b"wd:update" => {
                                let with_commit = crate::attr_parse_or_static(
                                    worker,
                                    &xml_util::attr2hash_map(&e),
                                    "commit",
                                ) == "1";
                                let inner_xml =
                                    self.parse(worker, reader, "wd:update", include_adaptor)?;
                                let mut inner_reader =
                                    Reader::from_str(std::str::from_utf8(&inner_xml).unwrap());
                                inner_reader.check_end_names(false);
                                let updates =
                                    update::make_update_struct(self, &mut inner_reader, worker);
                                if let Some(mut session) = self.sessions.last_mut() {
                                    self.database
                                        .clone()
                                        .read()
                                        .unwrap()
                                        .update(&mut session, updates)?;
                                    if with_commit {
                                        self.database
                                            .clone()
                                            .write()
                                            .unwrap()
                                            .commit(&mut session)?;
                                    }
                                }
                            }
                            b"wd:search" => {
                                let attr = xml_util::attr2hash_map(&e);
                                let name = crate::attr_parse_or_static(worker, &attr, "name");
                                let collection_name =
                                    crate::attr_parse_or_static(worker, &attr, "collection");
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
                            b"wd:stack" => {
                                if let Ok(Some(var)) = e.try_get_attribute(b"var") {
                                    if let Ok(var) = std::str::from_utf8(&var.value) {
                                        let _ = worker.execute_script(
                                            "stack.push",
                                            &("wd.stack.push({".to_owned() + var + "});"),
                                        );
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
                                    &xml_util::outer(&next, reader),
                                    worker,
                                    include_adaptor,
                                )?);
                            }
                            b"wd:for" => {
                                r.append(&mut process::r#for(
                                    self,
                                    &e,
                                    &xml_util::outer(&next, reader),
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
                    Event::PI(ref e) => {
                        Self::run_script(worker, e.unescape().expect("Error!"));
                    }
                    Event::Empty(ref e) => {
                        let name = e.name();
                        let name = name.as_ref();
                        match name {
                            b"wd:print" => {
                                r.append(
                                    &mut crate::attr_parse_or_static(
                                        worker,
                                        &xml_util::attr2hash_map(e),
                                        "value",
                                    )
                                    .as_bytes()
                                    .to_vec(),
                                );
                            }
                            b"wd:include" => {
                                let src = crate::attr_parse_or_static(
                                    worker,
                                    &xml_util::attr2hash_map(e),
                                    "src",
                                );
                                let xml = include_adaptor.include(&src);
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
                                if name == b"wd:stack" {
                                    let _ = worker.execute_script("stack.pop", "wd.stack.pop();");
                                } else if name == b"wd:session" {
                                    self.sessions.pop();
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
                            html_attr.push_str(&crate::eval_result(scope, value));
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
