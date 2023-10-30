use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use parking_lot::Mutex;

use crate::IncludeAdaptor;

pub struct WildDocState {
    cache_dir: PathBuf,
    include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
}

impl WildDocState {
    pub fn new(
        cache_dir: PathBuf,
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
    ) -> Self {
        Self {
            cache_dir,
            include_adaptor,
        }
    }

    #[inline(always)]
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    #[inline(always)]
    pub fn include_adaptor(&self) -> &Mutex<Box<dyn IncludeAdaptor + Send>> {
        &self.include_adaptor
    }
}
