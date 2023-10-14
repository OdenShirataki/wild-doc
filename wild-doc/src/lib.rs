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
    sync::Arc,
};

use anyhow::Result;
use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};

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
        relation_allocation_lot: u32,
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
                relation_allocation_lot,
            ))),
            default_include_adaptor: Arc::new(Mutex::new(default_include_adaptor)),
            cache_dir,
        }
    }

    pub fn database(&self) -> &RwLock<SessionDatabase> {
        &self.database
    }

    fn setup_scripts(
        &mut self,
        state: Arc<WildDocState>,
    ) -> Result<hashbrown::HashMap<String, Box<dyn WildDocScript>>> {
        let mut scripts: hashbrown::HashMap<String, Box<dyn WildDocScript>> =
            hashbrown::HashMap::new();

        scripts.insert("var".to_owned(), Box::new(Var::new(Arc::clone(&state))?));

        #[cfg(feature = "js")]
        scripts.insert("js".to_owned(), Box::new(Deno::new(Arc::clone(&state))?));

        #[cfg(feature = "py")]
        scripts.insert("py".to_owned(), Box::new(WdPy::new(Arc::clone(&state))?));

        Ok(scripts)
    }
    fn run_inner(
        &mut self,
        xml: &[u8],
        input_json: &[u8],
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
    ) -> Result<WildDocResult> {
        let global = Arc::new(RwLock::new(WildDocValue::Object(IndexMap::new())));

        let state = Arc::new(WildDocState::new(
            vec![hashbrown::HashMap::from([
                (
                    b"input".to_vec(),
                    Arc::new(RwLock::new(WildDocValue::from(
                        serde_json::from_slice(input_json).unwrap_or(serde_json::json!({})),
                    ))),
                ),
                (b"global".to_vec(), Arc::clone(&global)),
            ])],
            self.cache_dir.clone(),
            include_adaptor,
        ));
        
        let body = tokio::runtime::Runtime::new()?.block_on(
            Parser::new(
                Arc::clone(&self.database),
                self.setup_scripts(Arc::clone(&state))?,
                state,
            )?
            .parse(xml),
        )?;

        let options = match global.read().deref() {
            WildDocValue::Object(o) => o.get("result_options"),
            _ => None,
        }
        .cloned();
        Ok(WildDocResult { body, options })
    }
    pub fn run(&mut self, xml: &[u8], input_json: &[u8]) -> Result<WildDocResult> {
        self.run_inner(xml, input_json, Arc::clone(&self.default_include_adaptor))
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
