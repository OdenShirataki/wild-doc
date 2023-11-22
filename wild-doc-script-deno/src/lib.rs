pub mod module_loader;

use std::{
    ffi::c_void,
    path::{Path, PathBuf},
    sync::Arc,
};

use deno_runtime::{
    deno_core::{self, anyhow::Result, serde_v8, ModuleCode, ModuleSpecifier},
    deno_napi::v8::{self, HandleScope},
    permissions::PermissionsContainer,
    worker::{MainWorker, WorkerOptions},
};

use parking_lot::Mutex;
use wild_doc_script::{
    async_trait, serde_json, IncludeAdaptor, Stack, Vars, WildDocScript, WildDocValue,
};

use module_loader::WdModuleLoader;

pub struct Deno {
    worker: MainWorker,
}

fn wdmap2v8obj<'s>(wdv: &Vars, scope: &'s mut HandleScope) -> v8::Local<'s, v8::Object> {
    let root = v8::Object::new(scope);
    let mut obj_vars: Vec<(v8::Local<v8::Object>, &Vars)> = vec![(root, wdv)];

    loop {
        if let Some((current_obj, current_values)) = obj_vars.pop() {
            for (key, value) in current_values.into_iter() {
                if let Some(v8_key) = v8::String::new(scope, key) {
                    match value {
                        WildDocValue::Binary(v) => {
                            let ab = v8::ArrayBuffer::with_backing_store(
                                scope,
                                &v8::ArrayBuffer::new_backing_store_from_vec(v.to_owned())
                                    .make_shared(),
                            );
                            if let Some(v) = v8::Uint8Array::new(scope, ab, 0, ab.byte_length()) {
                                current_obj.set(scope, v8_key.into(), v.into());
                            }
                        }
                        WildDocValue::Object(map) => {
                            let new_obj = v8::Object::new(scope);
                            obj_vars.push((new_obj, map));
                            current_obj.set(scope, v8_key.into(), new_obj.into());
                        }
                        _ => {
                            if let Ok(v) = serde_v8::to_v8(scope, value) {
                                current_obj.set(scope, v8_key.into(), v.into());
                            }
                        }
                    }
                }
            }
        } else {
            break;
        }
    }

    root
}

fn wd2v8<'s>(wdv: &WildDocValue, scope: &'s mut HandleScope) -> Option<v8::Local<'s, v8::Value>> {
    match wdv {
        WildDocValue::Binary(v) => {
            let ab = v8::ArrayBuffer::with_backing_store(
                scope,
                &v8::ArrayBuffer::new_backing_store_from_vec(v.to_owned()).make_shared(),
            );
            if let Some(v) = v8::Uint8Array::new(scope, ab, 0, ab.byte_length()) {
                return Some(v.into());
            }
        }
        WildDocValue::Object(map) => {
            return Some(wdmap2v8obj(map, scope).into());
        }
        _ => {
            if let Ok(r) = serde_v8::to_v8(scope, wdv) {
                return Some(r.into());
            }
        }
    }

    None
}

fn v(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    if let Some(stack) = v8::String::new(scope, "wd.stack")
        .and_then(|code| v8::Script::compile(scope, code, None))
        .and_then(|v| v.run(scope))
    {
        let key = args
            .get(0)
            .to_string(scope)
            .unwrap()
            .to_rust_string_lossy(scope);

        let stack = unsafe { &*(v8::Local::<v8::External>::cast(stack).value() as *const Stack) };
        if let Some(v) = stack.get(&key) {
            if let Some(v) = wd2v8(v, scope) {
                retval.set(v);
            }
        }
    }
}

fn get_contents(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    if let Some(include_adaptor) = v8::String::new(scope, "wd.include_adaptor")
        .and_then(|code| v8::Script::compile(scope, code, None))
        .and_then(|v| v.run(scope))
    {
        let include_adaptor = unsafe {
            &*(v8::Local::<v8::External>::cast(include_adaptor).value()
                as *const Mutex<Box<dyn IncludeAdaptor + Send>>)
        };
        let filename = args
            .get(0)
            .to_string(scope)
            .unwrap()
            .to_rust_string_lossy(scope);
        if let Some(contents) = include_adaptor.lock().include(Path::new(filename.as_str())) {
            if let Ok(r) = serde_v8::to_v8(scope, contents) {
                retval.set(r.into());
            }
        }
    }
}

#[async_trait(?Send)]
impl WildDocScript for Deno {
    fn new(
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
        cache_dir: PathBuf,
        stack: &Stack,
    ) -> Result<Self> {
        v8::V8::set_flags_from_string("--stack_size=10240");
        let mut worker = MainWorker::bootstrap_from_options(
            deno_core::resolve_url("wd://main").unwrap(),
            PermissionsContainer::allow_all(),
            WorkerOptions {
                module_loader: WdModuleLoader::new(cache_dir),
                ..Default::default()
            },
        );
        worker.js_runtime.execute_script_static(
            "<anon>",
            r#"wd={
    general:{}
};"#,
        )?;

        if let Ok(wd) = worker.js_runtime.execute_script_static("<anon>", "wd") {
            let scope = &mut worker.js_runtime.handle_scope();
            if let (
                Some(wd),
                Some(v8str_get_contents),
                Some(v8func_get_contents),
                Some(v8str_include_adaptor),
                Some(v8str_stack),
                Some(v8str_v),
                Some(v8func_v),
            ) = (
                v8::Local::new(scope, wd).to_object(scope),
                v8::String::new(scope, "get_contents"),
                v8::Function::new(scope, get_contents),
                v8::String::new(scope, "include_adaptor"),
                v8::String::new(scope, "stack"),
                v8::String::new(scope, "v"),
                v8::Function::new(scope, v),
            ) {
                let v8ext_include_adaptor = v8::External::new(
                    scope,
                    include_adaptor.as_ref() as *const Mutex<Box<dyn IncludeAdaptor + Send>>
                        as *mut c_void,
                );
                wd.set(
                    scope,
                    v8str_include_adaptor.into(),
                    v8ext_include_adaptor.into(),
                );

                let v8ext_stack = v8::External::new(scope, stack as *const Stack as *mut c_void);
                wd.set(scope, v8str_stack.into(), v8ext_stack.into());

                wd.set(scope, v8str_get_contents.into(), v8func_get_contents.into());
                wd.set(scope, v8str_v.into(), v8func_v.into());
            }
        }
        Ok(Self { worker })
    }

    async fn evaluate_module(&mut self, file_name: &str, src: &str, _: &Stack) -> Result<()> {
        let mod_id = self
            .worker
            .js_runtime
            .load_side_module(
                &(ModuleSpecifier::parse(&("wd://script".to_owned() + file_name))?),
                Some(String::from_utf8(src.into())?.into()),
            )
            .await?;
        self.worker.evaluate_module(mod_id).await?;
        Ok(())
    }

    async fn eval(&mut self, code: &str, _: &Stack) -> Result<WildDocValue> {
        if let Ok(v) = self
            .worker
            .execute_script("<anon>", ModuleCode::from(code.to_owned()))
        {
            let scope = &mut self.worker.js_runtime.handle_scope();
            let v = v8::Local::new(scope, v);
            if v.is_array_buffer_view() {
                if let Ok(a) = v8::Local::<v8::ArrayBufferView>::try_from(v) {
                    if let Some(b) = a.buffer(scope) {
                        if let Some(d) = b.data() {
                            return Ok(WildDocValue::Binary(
                                unsafe {
                                    std::slice::from_raw_parts::<u8>(
                                        d.as_ptr() as *const u8,
                                        b.byte_length(),
                                    )
                                }
                                .into(),
                            ));
                        }
                    }
                }
            } else {
                if let Ok(serv) = serde_v8::from_v8::<serde_json::Value>(scope, v) {
                    return Ok(serv.into());
                }
            }
        }

        Ok(WildDocValue::Null)
    }
}
