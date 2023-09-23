mod include;
mod parser;
mod script;
mod xml_util;

pub use include::IncludeLocal;
pub use semilattice_database_session::DataOption;

use std::{
    collections::HashMap,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use anyhow::Result;

use semilattice_database_session::SessionDatabase;

use wild_doc_script::{IncludeAdaptor, WildDocScript, WildDocState, WildDocValue};

use parser::Parser;
use script::Var;

#[cfg(feature = "js")]
use wild_doc_script_deno::Deno;

#[cfg(feature = "py")]
use wild_doc_script_python::WdPy;

pub struct WildDocResult {
    body: Vec<u8>,
    options: Option<WildDocValue>,
}
impl WildDocResult {
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    pub fn options(&self) -> &Option<WildDocValue> {
        &self.options
    }
}

pub struct WildDoc {
    database: Arc<RwLock<SessionDatabase>>,
    default_include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
    cache_dir: PathBuf,
}
impl WildDoc {
    pub fn new<P: AsRef<Path>>(
        dir: P,
        default_include_adaptor: Box<dyn IncludeAdaptor + Send>,
        collection_settings: Option<HashMap<String, DataOption>>,
    ) -> Self {
        let dir = dir.as_ref();
        let mut cache_dir = dir.to_path_buf();
        cache_dir.push("modules");
        if !cache_dir.exists() {
            std::fs::create_dir_all(&cache_dir).unwrap();
        }
        Self {
            database: Arc::new(RwLock::new(SessionDatabase::new(
                dir.into(),
                collection_settings,
            ))),
            default_include_adaptor: Arc::new(Mutex::new(default_include_adaptor)),
            cache_dir,
        }
    }

    fn setup_scripts(
        &mut self,
        state: WildDocState,
    ) -> Result<HashMap<String, Arc<Mutex<dyn WildDocScript>>>> {
        let mut scripts: HashMap<String, Arc<Mutex<dyn WildDocScript>>> = HashMap::new();

        scripts.insert(
            "var".to_owned(),
            Arc::new(Mutex::new(Var::new(state.clone())?)),
        );

        #[cfg(feature = "js")]
        scripts.insert(
            "js".to_owned(),
            Arc::new(Mutex::new(Deno::new(state.clone())?)),
        );

        #[cfg(feature = "py")]
        scripts.insert(
            "py".to_owned(),
            Arc::new(Mutex::new(WdPy::new(state.clone())?)),
        );
        Ok(scripts)
    }
    fn run_inner(
        &mut self,
        xml: &[u8],
        input_json: &[u8],
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
    ) -> Result<WildDocResult> {
        let mut vars = HashMap::new();

        let input =
            WildDocValue::from(serde_json::from_slice(input_json).unwrap_or(serde_json::json!({})));
        vars.insert(b"input".to_vec(), Arc::new(RwLock::new(input)));

        let global = Arc::new(RwLock::new(WildDocValue::from(serde_json::json!({}))));
        vars.insert(b"global".to_vec(), Arc::clone(&global));

        let stack = Arc::new(RwLock::new(vec![vars]));

        let state = WildDocState::new(stack.clone(), self.cache_dir.clone(), include_adaptor);
        let scripts = Arc::new(self.setup_scripts(state.clone())?);

        let body = Parser::new(self.database.clone(), scripts.clone(), state)?.parse(xml)?;

        let options = match global.read().unwrap().deref() {
            WildDocValue::Object(o) => o.get("result_options"),
            _ => None,
        }
        .cloned();
        Ok(WildDocResult { body, options })
    }
    pub fn run(&mut self, xml: &[u8], input_json: &[u8]) -> Result<WildDocResult> {
        self.run_inner(xml, input_json, self.default_include_adaptor.clone())
    }
    pub fn run_with_include_adaptor(
        &mut self,
        xml: &[u8],
        input_json: &[u8],
        include_adaptor: Box<dyn IncludeAdaptor + Send>,
    ) -> Result<WildDocResult> {
        self.run_inner(xml, input_json, Arc::new(Mutex::new(include_adaptor)))
    }
}
