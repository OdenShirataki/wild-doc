use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use indexmap::IndexMap;
use parking_lot::Mutex;

use crate::{IncludeAdaptor, VarsStack, WildDocValue};

pub struct WildDocState {
    stack: Mutex<VarsStack>,
    global: Mutex<IndexMap<String, Arc<WildDocValue>>>,
    cache_dir: PathBuf,
    include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
}

impl WildDocState {
    pub fn new(
        initial_stack: VarsStack,
        cache_dir: PathBuf,
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
    ) -> Self {
        Self {
            stack: Mutex::new(initial_stack),
            global: Mutex::new(IndexMap::new()),
            cache_dir,
            include_adaptor,
        }
    }

    #[inline(always)]
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    #[inline(always)]
    pub fn stack(&self) -> &Mutex<VarsStack> {
        &self.stack
    }

    #[inline(always)]
    pub fn global(&self) -> &Mutex<IndexMap<String, Arc<WildDocValue>>> {
        &self.global
    }

    #[inline(always)]
    pub fn include_adaptor(&self) -> &Mutex<Box<dyn IncludeAdaptor + Send>> {
        &self.include_adaptor
    }
}
