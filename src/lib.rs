use deno_runtime::{deno_core::v8, worker::MainWorker};
use quick_xml::{events::Event, Reader};
use semilattice_database::Database;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

pub use deno_runtime::deno_core::error::AnyError;

mod script;
use script::Script;

mod xml_util;
use xml_util::XmlAttr;

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
    database: Arc<RwLock<Database>>,
    default_include_adaptor: T,
    module_cache_dir: PathBuf,
}
impl<T: IncludeAdaptor> WildDoc<T> {
    pub fn new<P: AsRef<Path>>(dir: P, default_include_adaptor: T) -> io::Result<Self> {
        let dir = dir.as_ref();
        let mut module_cache_dir = dir.to_path_buf();
        module_cache_dir.push("modules");
        if !module_cache_dir.exists() {
            std::fs::create_dir_all(&module_cache_dir)?;
        }
        Ok(Self {
            database: Arc::new(RwLock::new(Database::new(dir)?)),
            default_include_adaptor,
            module_cache_dir,
        })
    }

    fn run_inner(
        database: Arc<RwLock<Database>>,
        xml: &str,
        input_json: &str,
        include_adaptor: &mut impl IncludeAdaptor,
        module_cache_dir: &PathBuf,
    ) -> Result<WildDocResult, AnyError> {
        let mut reader = Reader::from_str(xml);
        reader.check_end_names(false);
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    if e.name().as_ref() == b"wd" {
                        let mut script = Script::new(database, module_cache_dir.clone());
                        return script.parse_xml(input_json, &mut reader, include_adaptor);
                    }
                }
                _ => {
                    return Ok(WildDocResult {
                        body: xml.into(),
                        options_json: "".to_string(),
                    });
                }
            }
        }
    }
    pub fn run(&mut self, xml: &str, input_json: &str) -> Result<WildDocResult, AnyError> {
        Self::run_inner(
            self.database.clone(),
            xml,
            input_json,
            &mut self.default_include_adaptor,
            &self.module_cache_dir,
        )
    }
    pub fn run_specify_include_adaptor(
        &mut self,
        xml: &str,
        input_json: &str,
        include_adaptor: &mut impl IncludeAdaptor,
    ) -> Result<WildDocResult, AnyError> {
        Self::run_inner(
            self.database.clone(),
            xml,
            input_json,
            include_adaptor,
            &self.module_cache_dir,
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

fn eval_result_string(scope: &mut v8::HandleScope, value: &str) -> String {
    if let Some(v8_value) = v8::String::new(scope, value)
        .and_then(|code| v8::Script::compile(scope, code, None))
        .and_then(|v| v.run(scope))
        .and_then(|v| v.to_string(scope))
    {
        v8_value.to_rust_string_lossy(scope)
    } else {
        "".to_string()
    }
}

fn attr_parse_or_static(worker: &mut MainWorker, attr: &XmlAttr, key: &str) -> Vec<u8> {
    let wdkey = "wd:".to_owned() + key;
    if let Some(value) = attr.get(&wdkey) {
        if let Ok(value) = std::str::from_utf8(value) {
            return crate::eval_result(&mut worker.js_runtime.handle_scope(), value);
        }
    } else if let Some(value) = attr.get(key) {
        return value.to_vec();
    }
    vec![]
}

fn attr_parse_or_static_string(worker: &mut MainWorker, attr: &XmlAttr, key: &str) -> String {
    let wdkey = "wd:".to_owned() + key;
    if let Some(value) = attr.get(&wdkey) {
        if let Ok(value) = std::str::from_utf8(value) {
            return crate::eval_result_string(&mut worker.js_runtime.handle_scope(), value);
        }
    } else if let Some(value) = attr.get(key) {
        if let Ok(str) = std::string::String::from_utf8(value.to_vec()) {
            return str;
        }
    }
    "".to_owned()
}
