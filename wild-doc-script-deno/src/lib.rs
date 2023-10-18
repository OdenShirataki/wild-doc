pub mod module_loader;

use std::{ffi::c_void, ops::Deref, path::Path, sync::Arc};

use deno_runtime::{
    deno_core::{self, anyhow::Result, serde_v8, ModuleSpecifier},
    deno_napi::v8::{self, HandleScope, NewStringType, PropertyAttribute},
    permissions::PermissionsContainer,
    worker::{MainWorker, WorkerOptions},
};
use indexmap::IndexMap;
use parking_lot::Mutex;

use wild_doc_script::{
    async_trait, serde_json, IncludeAdaptor, WildDocScript, WildDocState, WildDocValue,
};

use module_loader::WdModuleLoader;

pub struct Deno {
    worker: Mutex<MainWorker>,
}

fn wdmap2v8obj<'s>(
    wdv: &IndexMap<String, WildDocValue>,
    scope: &'s mut HandleScope,
) -> v8::Local<'s, v8::Object> {
    let root = v8::Object::new(scope);
    let mut obj_stack: Vec<(v8::Local<v8::Object>, &IndexMap<String, WildDocValue>)> =
        vec![(root, wdv)];

    loop {
        if let Some((current_obj, current_values)) = obj_stack.pop() {
            for (key, value) in current_values {
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
                            obj_stack.push((new_obj, map));
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
            let obj = wdmap2v8obj(map, scope);
            return Some(obj.into());
        }
        _ => {
            if let Ok(r) = serde_v8::to_v8(scope, wdv) {
                return Some(r.into());
            }
        }
    }

    None
}

#[async_trait(?Send)]
impl WildDocScript for Deno {
    fn new(state: Arc<WildDocState>) -> Result<Self> {
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
                            include_adaptor.lock().include(Path::new(filename.as_str()))
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
                    if let Some(state) = v8::String::new(scope, "wd.state")
                        .and_then(|code| v8::Script::compile(scope, code, None))
                        .and_then(|v| v.run(scope))
                    {
                        let state = unsafe {
                            &*(v8::Local::<v8::External>::cast(state).value()
                                as *const WildDocState)
                        };
                        let key = args
                            .get(0)
                            .to_string(scope)
                            .unwrap()
                            .to_rust_string_lossy(scope);
                        if key == "global" {
                            let r = wdmap2v8obj(state.global().lock().deref(), scope);
                            retval.set(r.into());
                        } else {
                            for stack in state.stack().lock().iter().rev() {
                                if let Some(v) = stack.get(key.as_bytes()) {
                                    if let Some(v) = wd2v8(v, scope) {
                                        retval.set(v);
                                    }
                                    break;
                                }
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
                Some(v8str_state),
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
                v8::String::new(scope, "state"),
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

                let v8ext_state =
                    v8::External::new(scope, state.as_ref() as *const WildDocState as *mut c_void);
                wd.define_own_property(
                    scope,
                    v8str_state.into(),
                    v8ext_state.into(),
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
        Ok(Self {
            worker: Mutex::new(worker),
        })
    }

    async fn evaluate_module(&self, file_name: &str, src: &[u8]) -> Result<()> {
        let mut worker = self.worker.lock();
        let mod_id = worker
            .js_runtime
            .load_side_module(
                &(ModuleSpecifier::parse(&("wd://script".to_owned() + file_name))?),
                Some(String::from_utf8(src.to_vec())?.into()),
            )
            .await?;
        worker.evaluate_module(mod_id).await?;
        Ok(())
    }

    async fn eval(&self, code: &[u8]) -> Result<WildDocValue> {
        let mut worker = self.worker.lock();
        let scope = &mut worker.js_runtime.handle_scope();

        let code = std::str::from_utf8(code)?;

        if let Some(v) = v8::String::new_from_one_byte(
            scope,
            ("(".to_owned() + code + ")").as_bytes(),
            NewStringType::Normal,
        )
        .and_then(|code| v8::Script::compile(scope, code, None))
        .and_then(|v| v.run(scope))
        {
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
                                .to_vec(),
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
