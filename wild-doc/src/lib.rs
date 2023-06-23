mod deno;
mod include;
mod parser;
mod xml_util;

use deno_runtime::deno_core::serde_json;
pub use include::IncludeLocal;
pub use semilattice_database_session::anyhow;

use std::{
    collections::HashMap,
    io::{self},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use wild_doc_script::{IncludeAdaptor, VarsStack, WildDocScript, WildDocState};

#[cfg(feature = "py")]
use wild_doc_script_python::WdPy;

use anyhow::Result;
use semilattice_database_session::SessionDatabase;

use parser::Parser;

use deno::Deno;

pub struct WildDocResult {
    body: Vec<u8>,
    options_json: Option<serde_json::Value>,
}
impl WildDocResult {
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    pub fn options_json(&self) -> &Option<serde_json::Value> {
        &self.options_json
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
    ) -> io::Result<Self> {
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

    fn setup_scripts(
        &mut self,
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
        stack: Arc<RwLock<VarsStack>>,
    ) -> Result<HashMap<String, Arc<Mutex<dyn WildDocScript>>>> {
        let state = WildDocState::new(stack.clone(), self.cache_dir.clone(), include_adaptor);
        let mut scripts: HashMap<String, Arc<Mutex<dyn WildDocScript>>> = HashMap::new();

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
    pub fn run(&mut self, xml: &[u8], input_json: &[u8]) -> Result<WildDocResult> {
        let stack = Arc::new(RwLock::new(vec![]));
        let scripts = self.setup_scripts(self.default_include_adaptor.clone(), stack.clone())?;
        Parser::new(
            self.database.clone(),
            scripts,
            WildDocState::new(
                stack.clone(),
                self.cache_dir.clone(),
                self.default_include_adaptor.clone(),
            ),
        )?
        .parse_xml(input_json, xml)
    }
    pub fn run_specify_include_adaptor(
        &mut self,
        xml: &[u8],
        input_json: &[u8],
        include_adaptor: Box<dyn IncludeAdaptor + Send>,
    ) -> Result<WildDocResult> {
        let stack = Arc::new(RwLock::new(vec![]));
        let include_adaptor = Arc::new(Mutex::new(include_adaptor));
        let state = WildDocState::new(
            stack.clone(),
            self.cache_dir.clone(),
            include_adaptor.clone(),
        );
        Parser::new(
            self.database.clone(),
            self.setup_scripts(include_adaptor, stack.clone())?,
            state,
        )?
        .parse_xml(input_json, xml)
    }
}

pub(crate) fn quot_unescape(value: &[u8]) -> String {
    let str = unsafe { std::str::from_utf8_unchecked(value) };
    str.replace("&#039;", "'").replace("&quot;", "\"")
}
