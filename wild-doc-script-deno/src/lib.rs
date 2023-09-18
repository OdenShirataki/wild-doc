pub mod module_loader;

use std::{
    ffi::c_void,
    ops::{Deref, DerefMut},
    sync::{Mutex, RwLock},
};

use bson::Bson;
use deno_runtime::{
    deno_core::{self, anyhow::Result, serde_v8, ModuleSpecifier},
    deno_napi::v8::{self, NewStringType, PropertyAttribute},
    permissions::PermissionsContainer,
    worker::{MainWorker, WorkerOptions},
};

use wild_doc_script::{IncludeAdaptor, VarsStack, WildDocScript, WildDocState};

use module_loader::WdModuleLoader;

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
impl WildDocScript for Deno {
    fn new(state: WildDocState) -> Result<Self> {
        let mut worker = MainWorker::bootstrap_from_options(
            deno_core::resolve_url("wd://main").unwrap(),
            PermissionsContainer::allow_all(),
            WorkerOptions {
                module_loader: WdModuleLoader::new(state.cache_dir().to_owned()),
                ..Default::default()
            },
        );
        worker.execute_script(
            "new",
            deno_core::FastString::from_static(
                r#"wd={
    general:{}
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
                                as *mut Mutex<Box<dyn IncludeAdaptor + Send>>)
                        };
                        if let Some(contents) =
                            include_adaptor.lock().unwrap().include(filename.into())
                        {
                            if let Ok(r) = serde_v8::to_v8(scope, contents) {
                                retval.set(r.into());
                            }
                        }
                    }
                },
            );

            let func_v = v8::Function::new(
                scope,
                |scope: &mut v8::HandleScope,
                 args: v8::FunctionCallbackArguments,
                 mut retval: v8::ReturnValue| {
                    if let Some(stack) = v8::String::new(scope, "wd.stack")
                        .and_then(|code| v8::Script::compile(scope, code, None))
                        .and_then(|v| v.run(scope))
                    {
                        let stack = unsafe {
                            &*(v8::Local::<v8::External>::cast(stack).value()
                                as *const RwLock<VarsStack>)
                        };
                        let key = args
                            .get(0)
                            .to_string(scope)
                            .unwrap()
                            .to_rust_string_lossy(scope);

                        for stack in stack.read().unwrap().iter().rev() {
                            if let Some(v) = stack.get(key.as_bytes()) {
                                if let Ok(r) = serde_v8::to_v8(scope, v) {
                                    retval.set(r.into());
                                }
                                break;
                            }
                        }
                    }
                },
            );

            if let (
                Some(wd),
                Some(v8str_include_adaptor),
                Some(v8str_get_contents),
                Some(v8func_get_contents),
                Some(v8str_stack),
                Some(v8str_v),
                Some(v8func_v),
            ) = (
                v8::String::new(scope, "wd")
                    .and_then(|code| v8::Script::compile(scope, code, None))
                    .and_then(|v| v.run(scope))
                    .and_then(|v| v8::Local::<v8::Object>::try_from(v).ok()),
                v8::String::new(scope, "include_adaptor"),
                v8::String::new(scope, "get_contents"),
                func_get_contents,
                v8::String::new(scope, "stack"),
                v8::String::new(scope, "v"),
                func_v,
            ) {
                let v8ext_include_adaptor = v8::External::new(
                    scope,
                    state.include_adaptor() as *const Mutex<Box<dyn IncludeAdaptor + Send>>
                        as *mut c_void,
                );
                wd.define_own_property(
                    scope,
                    v8str_include_adaptor.into(),
                    v8ext_include_adaptor.into(),
                    PropertyAttribute::READ_ONLY,
                );

                let v8ext_stack = v8::External::new(
                    scope,
                    state.stack().as_ref() as *const RwLock<VarsStack> as *mut c_void,
                );
                wd.define_own_property(
                    scope,
                    v8str_stack.into(),
                    v8ext_stack.into(),
                    PropertyAttribute::READ_ONLY,
                );

                wd.define_own_property(
                    scope,
                    v8str_get_contents.into(),
                    v8func_get_contents.into(),
                    PropertyAttribute::READ_ONLY,
                );
                wd.define_own_property(
                    scope,
                    v8str_v.into(),
                    v8func_v.into(),
                    PropertyAttribute::READ_ONLY,
                );
            }
        }
        Ok(Self { worker })
    }
    fn evaluate_module(&mut self, file_name: &str, src: &[u8]) -> Result<()> {
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
    fn eval(&mut self, code: &[u8]) -> Result<Bson> {
        let code = "(".to_owned() + std::str::from_utf8(code)? + ")";
        let scope = &mut self.js_runtime.handle_scope();

        if let Some(v) =
            v8::String::new_from_one_byte(scope, code.as_bytes(), NewStringType::Normal)
                .and_then(|code| v8::Script::compile(scope, code, None))
                .and_then(|v| v.run(scope))
        {
            if let Ok(serv) = serde_v8::from_v8::<Bson>(scope, v) {
                return Ok(serv);
            }
        }
        Ok(Bson::Null)
    }
}
