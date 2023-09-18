use std::{
    collections::HashMap,
    path::{PathBuf, Path},
    sync::{Arc, RwLock, Mutex},
};

use bson::Bson;

pub use anyhow;
pub use serde_json;

use anyhow::Result;

pub type Vars = HashMap<Vec<u8>, Arc<RwLock<Bson>>>;
pub type VarsStack = Vec<Vars>;

pub trait IncludeAdaptor {
    fn include(&mut self, path: PathBuf) -> Option<Arc<Vec<u8>>>;
}

pub trait WildDocScript {
    fn new(state: WildDocState) -> Result<Self>
    where
        Self: Sized;
    fn evaluate_module(&mut self, file_name: &str, src: &[u8]) -> Result<()>;
    fn eval(&mut self, code: &[u8]) -> Result<Bson>;
}

#[derive(Clone)]
pub struct WildDocState {
    stack: Arc<RwLock<VarsStack>>,
    cache_dir: PathBuf,
    include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
}

impl WildDocState {
    pub fn new(
        stack: Arc<RwLock<VarsStack>>,
        cache_dir: PathBuf,
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
    ) -> Self {
        Self {
            stack,
            cache_dir,
            include_adaptor,
        }
    }
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
    pub fn stack(&self) -> Arc<RwLock<VarsStack>> {
        self.stack.clone()
    }
    pub fn include_adaptor(&self) -> &Mutex<Box<dyn IncludeAdaptor + Send>> {
        &self.include_adaptor
    }
}
