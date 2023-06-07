use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use anyhow::Result;
use deno_runtime::{deno_core::v8, deno_napi::v8::NewStringType};
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
        Script::new(
            self.database.clone(),
            self.default_include_adaptor.clone(),
            self.cache_dir.clone(),
        )
        .parse_xml(input_json, xml)
    }
    pub fn run_specify_include_adaptor<I: IncludeAdaptor>(
        &mut self,
        xml: &[u8],
        input_json: &[u8],
        include_adaptor: I,
    ) -> Result<WildDocResult> {
        Script::new(
            self.database.clone(),
            Arc::new(Mutex::new(include_adaptor)),
            self.cache_dir.clone(),
        )
        .parse_xml(input_json, xml)
    }
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
