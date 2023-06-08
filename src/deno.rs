pub mod module_loader;

use std::{
    ffi::c_void,
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::{Mutex, RwLock},
};

use deno_runtime::{
    deno_core::{self, serde_v8, ModuleSpecifier},
    deno_napi::v8::{self, HandleScope, NewStringType, PropertyAttribute},
    permissions::PermissionsContainer,
    worker::{MainWorker, WorkerOptions},
};
use semilattice_database_session::SessionDatabase;

use self::module_loader::WdModuleLoader;

use crate::{anyhow::Result, IncludeAdaptor};

pub struct Deno {
    worker: MainWorker,
}
impl Deref for Deno {
    type Target = MainWorker;

    fn deref(&self) -> &Self::Target {
        &self.worker
    }
}
impl DerefMut for Deno {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.worker
    }
}
impl Deno {
    pub fn new<T: IncludeAdaptor>(
        database: &RwLock<SessionDatabase>,
        include_adaptor: &Mutex<T>,
        module_cache_dir: PathBuf,
    ) -> Result<Self> {
        let mut worker = MainWorker::bootstrap_from_options(
            deno_core::resolve_url("wd://main").unwrap(),
            PermissionsContainer::allow_all(),
            WorkerOptions {
                module_loader: WdModuleLoader::new(module_cache_dir),
                ..Default::default()
            },
        );
        worker.execute_script(
            "new",
            deno_core::FastString::from_static(
                r#"wd={
    general:{}
    ,stack:[]
    ,result_options:{}
    ,v:key=>{
        for(let i=wd.stack.length-1;i>=0;i--){
            if(wd.stack[i][key]!==void 0){
                return wd.stack[i][key];
            }
        }
    }
};"#,
            ),
        )?;
        {
            let scope = &mut worker.js_runtime.handle_scope();

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
                                as *mut Mutex<T>)
                        };
                        if let Some(contents) = include_adaptor.lock().unwrap().include(filename) {
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
                let v8ext_include_adaptor =
                    v8::External::new(scope, include_adaptor as *const Mutex<T> as *mut c_void);
                wd.define_own_property(
                    scope,
                    v8str_include_adaptor.into(),
                    v8ext_include_adaptor.into(),
                    PropertyAttribute::READ_ONLY,
                );

                let v8ext_script = v8::External::new(
                    scope,
                    database as *const RwLock<SessionDatabase> as *mut c_void,
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
        Ok(Self { worker })
    }
    pub fn input(&mut self, input_json: &[u8]) -> Result<()> {
        self.execute_script(
            "init",
            (r#"wd.input="#.to_owned()
                + (if input_json.len() > 0 {
                    std::str::from_utf8(input_json)?
                } else {
                    "{}"
                })
                + ";")
                .into(),
        )?;

        Ok(())
    }

    pub fn evaluate_module(&mut self, file_name: &str, src: &[u8]) -> Result<()> {
        deno_runtime::tokio_util::create_basic_runtime().block_on(async {
            let script_name = "wd://script".to_owned() + file_name;
            let mod_id = self
                .js_runtime
                .load_side_module(
                    &ModuleSpecifier::parse(&script_name)?,
                    Some(String::from_utf8(src.to_vec())?.into()),
                )
                .await?;
            MainWorker::evaluate_module(&mut self.worker, mod_id).await?;
            self.run_event_loop(false).await
        })
    }

    pub fn eval_json_string(&mut self, object_name: &[u8]) -> String {
        let scope = &mut self.js_runtime.handle_scope();
        if let Some(json) = v8::String::new_from_one_byte(scope, object_name, NewStringType::Normal)
            .and_then(|code| v8::Script::compile(scope, code, None))
            .and_then(|v| v.run(scope))
            .and_then(|v| v8::json::stringify(scope, v))
        {
            json.to_rust_string_lossy(scope)
        } else {
            String::new()
        }
    }

    pub fn eval_string(&mut self, code: &[u8]) -> String {
        let scope = &mut self.js_runtime.handle_scope();
        eval_result_string(scope, code)
    }
}

pub fn eval_result_string(scope: &mut v8::HandleScope, code: &[u8]) -> String {
    if let Some(v8_value) = v8::String::new_from_one_byte(scope, code, NewStringType::Normal)
        .and_then(|code| v8::Script::compile(scope, code, None))
        .and_then(|v| v.run(scope))
        .and_then(|v| v.to_string(scope))
    {
        v8_value.to_rust_string_lossy(scope)
    } else {
        "".to_string()
    }
}

pub fn get_wddb<'s>(scope: &mut v8::HandleScope<'s>) -> Option<&'s RwLock<SessionDatabase>> {
    if let Some(database) = v8::String::new(scope, "wd.database")
        .and_then(|code| v8::Script::compile(scope, code, None))
        .and_then(|v| v.run(scope))
    {
        Some(unsafe {
            &*(v8::Local::<v8::External>::cast(database).value() as *const RwLock<SessionDatabase>)
        })
    } else {
        None
    }
}

pub fn push_stack(scope: &mut HandleScope, obj: v8::Local<v8::Object>) {
    let context = scope.get_current_context();
    let scope = &mut v8::ContextScope::new(scope, context);
    if let (Some(v8str_wd), Some(v8str_stack)) = (
        v8::String::new(scope, "wd"),
        v8::String::new(scope, "stack"),
    ) {
        let global = context.global(scope);
        if let Some(wd) = global.get(scope, v8str_wd.into()) {
            if let Ok(wd) = v8::Local::<v8::Object>::try_from(wd) {
                if let Some(stack) = wd.get(scope, v8str_stack.into()) {
                    if let Ok(stack) = v8::Local::<v8::Array>::try_from(stack) {
                        stack.set_index(scope, stack.length(), obj.into());
                    }
                }
            }
        }
    }
}
