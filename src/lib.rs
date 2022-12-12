use std::sync::{Arc, RwLock};

use quick_xml::{events::Event, Reader};
use semilattice_database::Database;

mod script;
use script::Script;

mod xml_util;
use xml_util::XmlAttr;

use deno_runtime::{deno_core::v8, worker::MainWorker};

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
}
impl<T: IncludeAdaptor> WildDoc<T> {
    pub fn new(dir: &str, default_include_adaptor: T) -> Result<Self, std::io::Error> {
        Ok(Self {
            database: Arc::new(RwLock::new(Database::new(dir)?)),
            default_include_adaptor,
        })
    }
    pub fn run(&mut self, xml: &str, input_json: &str) -> Result<WildDocResult, std::io::Error> {
        let mut reader = Reader::from_str(xml);
        reader.check_end_names(false);
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    if e.name().as_ref() == b"wd" {
                        let mut script = Script::new(self.database.clone());
                        return script.parse_xml(
                            input_json,
                            &mut reader,
                            &mut self.default_include_adaptor,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    pub fn run_specify_include_adaptor(
        &mut self,
        xml: &str,
        input_json: &str,
        index_adaptor: &mut impl IncludeAdaptor,
    ) -> Result<WildDocResult, std::io::Error> {
        let mut reader = Reader::from_str(xml);
        reader.check_end_names(false);
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    if e.name().as_ref() == b"wd" {
                        let mut script = Script::new(self.database.clone());
                        return script.parse_xml(input_json, &mut reader, index_adaptor);
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
}

fn eval_result(scope: &mut v8::HandleScope, value: &str) -> String {
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

fn attr_parse_or_static(worker: &mut MainWorker, attr: &XmlAttr, key: &str) -> String {
    let wdkey = "wd:".to_owned() + key;
    if let Some(value) = attr.get(&wdkey) {
        if let Ok(value) = std::str::from_utf8(value) {
            crate::eval_result(&mut worker.js_runtime.handle_scope(), value)
        } else {
            "".to_owned()
        }
    } else if let Some(value) = attr.get(key) {
        if let Ok(value) = std::str::from_utf8(value) {
            value
        } else {
            ""
        }
        .to_owned()
    } else {
        "".to_owned()
    }
}
