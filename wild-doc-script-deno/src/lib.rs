pub mod module_loader;

use std::{
    ffi::c_void,
    num::{NonZeroI64, NonZeroU32},
    path::{Path, PathBuf},
    sync::Arc,
};

use deno_runtime::{
    deno_core::{self, anyhow::Result, serde_v8, ModuleSpecifier},
    deno_napi::v8::{self, HandleScope},
    permissions::PermissionsContainer,
    worker::{MainWorker, WorkerOptions},
};

use parking_lot::Mutex;
use wild_doc_script::{
    async_trait, serde_json, IncludeAdaptor, SearchResult, SessionSearchResult, Stack, Vars,
    WildDocScript, WildDocValue,
};

use module_loader::WdModuleLoader;

pub struct Deno {
    worker: MainWorker,
}

fn session_result2v8obj<'s>(
    result: &SessionSearchResult,
    scope: &'s mut HandleScope,
) -> v8::Local<'s, v8::Object> {
    let r: v8::Local<'_, v8::Object> = v8::Object::new(scope);

    if let (Some(v8str_inner), Some(v8str_rows), Some(v8str_join)) = (
        v8::String::new(scope, "inner"),
        v8::String::new(scope, "rows"),
        v8::String::new(scope, "join"),
    ) {
        let pkey = v8::Private::for_api(scope, v8str_inner.into());
        let v8ext_inner =
            v8::External::new(scope, result as *const SessionSearchResult as *mut c_void);
        r.set_private(scope, pkey.into(), v8ext_inner.into());

        r.set_accessor(
            scope,
            v8str_rows.into(),
            |scope: &mut v8::HandleScope,
             _: v8::Local<v8::Name>,
             args: v8::PropertyCallbackArguments,
             mut rv: v8::ReturnValue| {
                if let Some(key_inner) = v8::String::new(scope, "inner") {
                    let this = args.this();
                    let pkey = v8::Private::for_api(scope, key_inner.into());
                    if let Some(v8v) = this.get_private(scope, pkey) {
                        let inner = unsafe {
                            &*(v8::Local::<v8::External>::cast(v8v).value()
                                as *const SessionSearchResult)
                        };
                        let rows = inner.rows();
                        let v8rows = v8::Array::new(scope, rows.len() as i32);
                        let mut index = 0;
                        for i in rows {
                            let v8num = v8::BigInt::new_from_i64(scope, i.get());
                            v8rows.set_index(scope, index, v8num.into());
                            index += 1;
                        }
                        rv.set(v8rows.into());
                    }
                }
            },
        );
        if let Some(v8func_join) = v8::Function::new(
            scope,
            |scope: &mut v8::HandleScope,
             args: v8::FunctionCallbackArguments,
             mut rv: v8::ReturnValue| {
                if let (Some(v8str_inner), Some(row)) = (
                    v8::String::new(scope, "inner"),
                    args.get(1).to_uint32(scope),
                ) {
                    let row = row.value();
                    let this = args.this();
                    let pkey = v8::Private::for_api(scope, v8str_inner.into());
                    if let Some(v8v) = this.get_private(scope, pkey) {
                        let result = unsafe {
                            &*(v8::Local::<v8::External>::cast(v8v).value()
                                as *const SessionSearchResult)
                        };
                        let key = args.get(0).to_rust_string_lossy(scope);
                        if let Some(join) = result.join().get(&key) {
                            if let Some(row) = NonZeroI64::new(row.into()) {
                                if let Some(result) = join.get(&row) {
                                    let obj: v8::Local<'_, v8::Object> =
                                        session_result2v8obj(result, scope);
                                    rv.set(obj.into());
                                }
                            }
                        }
                    }
                }
            },
        ) {
            r.set(scope, v8str_join.into(), v8func_join.into());
        }
    }
    r
}

fn result2v8obj<'s>(
    result: &SearchResult,
    scope: &'s mut HandleScope,
) -> v8::Local<'s, v8::Object> {
    let r: v8::Local<'_, v8::Object> = v8::Object::new(scope);

    if let (Some(v8str_inner), Some(v8str_rows), Some(v8str_join)) = (
        v8::String::new(scope, "inner"),
        v8::String::new(scope, "rows"),
        v8::String::new(scope, "join"),
    ) {
        let pkey = v8::Private::for_api(scope, v8str_inner.into());
        let v8ext_inner = v8::External::new(scope, result as *const SearchResult as *mut c_void);
        r.set_private(scope, pkey.into(), v8ext_inner.into());

        r.set_accessor(
            scope,
            v8str_rows.into(),
            |scope: &mut v8::HandleScope,
             _: v8::Local<v8::Name>,
             args: v8::PropertyCallbackArguments,
             mut rv: v8::ReturnValue| {
                if let Some(key_inner) = v8::String::new(scope, "inner") {
                    let this = args.this();
                    let pkey = v8::Private::for_api(scope, key_inner.into());
                    if let Some(v8v) = this.get_private(scope, pkey) {
                        let inner = unsafe {
                            &*(v8::Local::<v8::External>::cast(v8v).value() as *const SearchResult)
                        };
                        let rows = inner.rows();
                        let v8rows = v8::Array::new(scope, rows.len() as i32);
                        let mut index = 0;
                        for i in rows {
                            let v8num = v8::Integer::new_from_unsigned(scope, i.get());
                            v8rows.set_index(scope, index, v8num.into());
                            index += 1;
                        }
                        rv.set(v8rows.into());
                    }
                }
            },
        );
        if let Some(v8func_join) = v8::Function::new(
            scope,
            |scope: &mut v8::HandleScope,
             args: v8::FunctionCallbackArguments,
             mut rv: v8::ReturnValue| {
                if let (Some(v8str_inner), Some(row)) = (
                    v8::String::new(scope, "inner"),
                    args.get(1).to_uint32(scope),
                ) {
                    let row = row.value();
                    let this = args.this();
                    let pkey = v8::Private::for_api(scope, v8str_inner.into());
                    if let Some(v8v) = this.get_private(scope, pkey) {
                        let result = unsafe {
                            &*(v8::Local::<v8::External>::cast(v8v).value() as *const SearchResult)
                        };
                        let key = args.get(0).to_rust_string_lossy(scope);
                        if let Some(join) = result.join().get(&key) {
                            if let Some(row) = NonZeroU32::new(row) {
                                if let Some(result) = join.get(&row) {
                                    let obj: v8::Local<'_, v8::Object> =
                                        result2v8obj(result, scope);
                                    rv.set(obj.into());
                                }
                            }
                        }
                    }
                }
            },
        ) {
            r.set(scope, v8str_join.into(), v8func_join.into());
        }
    }
    r
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
        WildDocValue::SearchResult(result) => {
            return Some(result2v8obj(result, scope).into());
        }
        WildDocValue::SessionSearchResult(result) => {
            return Some(session_result2v8obj(result, scope).into());
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
        if let Some(v) = stack.get(&Arc::new(key)) {
            if let Some(v) = wd2v8(v, scope) {
                retval.set(v);
            }
        }
    }
}

fn get_contents<I: IncludeAdaptor + Send>(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    if let Some(include_adaptor) = v8::String::new(scope, "wd.include_adaptor")
        .and_then(|code| v8::Script::compile(scope, code, None))
        .and_then(|v| v.run(scope))
    {
        let include_adaptor = unsafe {
            &*(v8::Local::<v8::External>::cast(include_adaptor).value() as *const Mutex<I>)
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
impl<I: IncludeAdaptor + Send> WildDocScript<I> for Deno {
    fn new(include_adaptor: Arc<Mutex<I>>, cache_dir: PathBuf, stack: &Stack) -> Result<Self> {
        v8::V8::set_flags_from_string("--stack_size=10240");
        let mut worker = MainWorker::bootstrap_from_options(
            deno_core::resolve_url("wd://main").unwrap(),
            PermissionsContainer::allow_all(),
            WorkerOptions {
                module_loader: WdModuleLoader::new(cache_dir),
                startup_snapshot: Some(deno_core::Snapshot::Static(include_bytes!(
                    "../runtime.bin"
                ))),
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
                v8::Function::new(scope, get_contents::<I>),
                v8::String::new(scope, "include_adaptor"),
                v8::String::new(scope, "stack"),
                v8::String::new(scope, "v"),
                v8::Function::new(scope, v),
            ) {
                let v8ext_include_adaptor = v8::External::new(
                    scope,
                    include_adaptor.as_ref() as *const Mutex<I> as *mut c_void,
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
        if let Ok(v) = self.worker.execute_script("<anon>", code.to_owned().into()) {
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
