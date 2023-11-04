mod include;
mod parser;
mod script;
mod xml_util;

pub use include::IncludeLocal;
pub use semilattice_database_session::DataOption;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use parking_lot::{Mutex, RwLock};

use semilattice_database_session::SessionDatabase;

use wild_doc_script::{IncludeAdaptor, Vars};

use parser::Parser;

pub struct WildDocResult {
    body: Vec<u8>,
    options: Vars,
}
impl WildDocResult {
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn options(&self) -> &Vars {
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

    fn run_inner(
        &self,
        xml: &[u8],
        input_json: &[u8],
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
    ) -> Result<WildDocResult> {
        let parser = Parser::new(Arc::clone(&self.database), include_adaptor, &self.cache_dir)?;

        let body = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(32)
            .build()?
            .block_on(
                parser.parse(
                    xml,
                    [(
                        "input".into(),
                        Arc::new(
                            serde_json::from_slice(input_json)
                                .unwrap_or(serde_json::json!({}))
                                .into(),
                        ),
                    )]
                    .into(),
                ),
            )?;

        let options = parser.result_options().lock().clone();

        Ok(WildDocResult { body, options })
    }

    pub fn run(&self, xml: &[u8], input_json: &[u8]) -> Result<WildDocResult> {
        self.run_inner(xml, input_json, Arc::clone(&self.default_include_adaptor))
    }

    pub fn run_with_include_adaptor(
        &self,
        xml: &[u8],
        input_json: &[u8],
        include_adaptor: Box<dyn IncludeAdaptor + Send>,
    ) -> Result<WildDocResult> {
        self.run_inner(xml, input_json, Arc::new(Mutex::new(include_adaptor)))
    }
}
