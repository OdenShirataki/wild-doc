use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use wild_doc_script::IncludeAdaptor;

pub struct IncludeLocal {
    dir: PathBuf,
    cache: HashMap<PathBuf, Arc<Vec<u8>>>,
}
impl IncludeLocal {
    #[inline(always)]
    pub fn new<P: AsRef<Path>>(dir: P) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            cache: HashMap::default(),
        }
    }
}
impl IncludeAdaptor for IncludeLocal {
    #[inline(always)]
    fn include(&mut self, path: PathBuf) -> Option<Arc<Vec<u8>>> {
        if !self.cache.contains_key(&path) {
            let mut file_path = self.dir.clone();
            file_path.push(&path);
            if let Ok(mut f) = std::fs::File::open(file_path) {
                let mut contents = Vec::new();
                if let Ok(_) = f.read_to_end(&mut contents) {
                    self.cache.insert(path.to_owned(), Arc::new(contents));
                }
            }
        }
        self.cache.get(&path).map(|v| v.clone())
    }
}
