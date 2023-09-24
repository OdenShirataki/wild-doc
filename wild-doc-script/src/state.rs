use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use crate::{IncludeAdaptor, VarsStack};

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

    #[inline(always)]
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    #[inline(always)]
    pub fn stack(&self) -> Arc<RwLock<VarsStack>> {
        self.stack.clone()
    }

    #[inline(always)]
    pub fn include_adaptor(&self) -> &Mutex<Box<dyn IncludeAdaptor + Send>> {
        &self.include_adaptor
    }
}
