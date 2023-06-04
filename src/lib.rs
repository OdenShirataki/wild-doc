use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use anyhow::Result;
use deno_runtime::{deno_core::v8, deno_napi::v8::NewStringType, worker::MainWorker};
use maybe_xml::token;
pub use semilattice_database_session::anyhow;
use semilattice_database_session::SessionDatabase;

mod script;
use script::Script;

mod xml_util;

mod include;
pub use include::{IncludeAdaptor, IncludeLocal};

pub struct WildDocResult {
    body: Vec<u8>,
    options_json: String,
}
impl WildDocResult {
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    pub fn options_json(&self) -> &str {
        &self.options_json
    }
}
pub struct WildDoc<T: IncludeAdaptor> {
    database: Arc<RwLock<SessionDatabase>>,
    default_include_adaptor: Arc<Mutex<T>>,
    cache_dir: PathBuf,
}
impl<T: IncludeAdaptor> WildDoc<T> {
    pub fn new<P: AsRef<Path>>(dir: P, default_include_adaptor: T) -> io::Result<Self> {
        let dir = dir.as_ref();
        let mut cache_dir = dir.to_path_buf();
        cache_dir.push("modules");
        if !cache_dir.exists() {
            std::fs::create_dir_all(&cache_dir)?;
        }
        Ok(Self {
            database: Arc::new(RwLock::new(SessionDatabase::new(dir)?)),
            default_include_adaptor: Arc::new(Mutex::new(default_include_adaptor)),
            cache_dir,
        })
    }

    pub fn run(&mut self, xml: &[u8], input_json: &[u8]) -> Result<WildDocResult> {
        Script::new(self.database.clone(), self.cache_dir.clone()).parse_xml(
            input_json,
            xml,
            self.default_include_adaptor.clone(),
        )
    }
    pub fn run_specify_include_adaptor<I: IncludeAdaptor>(
        &mut self,
        xml: &[u8],
        input_json: &[u8],
        include_adaptor: I,
    ) -> Result<WildDocResult> {
        Script::new(self.database.clone(), self.cache_dir.clone()).parse_xml(
            input_json,
            xml,
            Arc::new(Mutex::new(include_adaptor)),
        )
    }
}

fn eval_result(scope: &mut v8::HandleScope, var: &str) -> Vec<u8> {
    if let Some(v8_value) = v8::String::new(scope, var)
        .and_then(|code| v8::Script::compile(scope, code, None))
        .and_then(|v| v.run(scope))
    {
        if v8_value.is_uint8_array() {
            if let Ok(a) = v8::Local::<v8::Uint8Array>::try_from(v8_value) {
                if let Some(buf) = a.buffer(scope) {
                    let len = buf.byte_length();
                    if let Some(data) = buf.get_backing_store().data() {
                        let ptr = data.as_ptr() as *mut u8;
                        unsafe {
                            return core::slice::from_raw_parts(ptr, len).to_vec();
                        }
                    }
                }
            }
        } else {
            if let Some(string) = v8_value.to_string(scope) {
                return string.to_rust_string_lossy(scope).as_bytes().to_owned();
            }
        }
    }
    vec![]
}

fn eval_result_string(scope: &mut v8::HandleScope, value: &[u8]) -> String {
    if let Some(v8_value) = v8::String::new_from_one_byte(scope, value, NewStringType::Normal)
        .and_then(|code| v8::Script::compile(scope, code, None))
        .and_then(|v| v.run(scope))
        .and_then(|v| v.to_string(scope))
    {
        v8_value.to_rust_string_lossy(scope)
    } else {
        "".to_string()
    }
}

pub(crate) fn quot_unescape(value: &[u8]) -> String {
    let str = unsafe { std::str::from_utf8_unchecked(value) };
    str.replace("&#039;", "'").replace("&quot;", "\"")
}

fn attr2map<'a>(
    attributes: &'a Option<token::prop::Attributes>,
) -> HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)> {
    let mut map = HashMap::new();
    if let Some(attributes) = attributes {
        for a in attributes.iter() {
            let name = a.name();
            map.insert(
                name.local().as_bytes().to_vec(),
                (
                    if let Some(prefix) = name.namespace_prefix() {
                        Some(prefix.to_vec())
                    } else {
                        None
                    },
                    if let Some(value) = a.value() {
                        Some(value.to_vec())
                    } else {
                        None
                    },
                ),
            );
        }
    }
    map
}

fn attr_parse_or_static_string(
    worker: &mut MainWorker,
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    key: &[u8],
) -> String {
    if let Some((prefix, Some(value))) = attributes.get(key) {
        let prefix = if let Some(prefix) = prefix {
            prefix.as_slice()
        } else {
            b""
        };
        return if prefix == b"wd" {
            crate::eval_result_string(
                &mut worker.js_runtime.handle_scope(),
                quot_unescape(value).as_ref(),
            )
        } else {
            quot_unescape(value)
        };
    }
    "".to_owned()
}
fn attr_parse_or_static(
    worker: &mut MainWorker,
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    key: &[u8],
) -> Vec<u8> {
    if let Some((prefix, Some(value))) = attributes.get(key) {
        let prefix = if let Some(prefix) = prefix {
            prefix.as_slice()
        } else {
            b""
        };
        return if prefix == b"wd" {
            crate::eval_result(
                &mut worker.js_runtime.handle_scope(),
                quot_unescape(value).as_ref(),
            )
        } else {
            quot_unescape(value).as_bytes().to_vec()
        };
    }
    vec![]
}
