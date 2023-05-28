use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    rc::Rc,
};

pub trait IncludeAdaptor {
    fn include<P: AsRef<Path>>(&mut self, path: P) -> Option<Rc<Vec<u8>>>;
}
pub struct IncludeLocal {
    dir: PathBuf,
    cache: HashMap<PathBuf, Rc<Vec<u8>>>,
}
impl IncludeLocal {
    pub fn new<P: AsRef<Path>>(dir: P) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            cache: HashMap::default(),
        }
    }
}
impl IncludeAdaptor for IncludeLocal {
    fn include<P: AsRef<Path>>(&mut self, path: P) -> Option<Rc<Vec<u8>>> {
        let path = path.as_ref();
        if !self.cache.contains_key(path) {
            let mut file_path = self.dir.clone();
            file_path.push(&path);
            if let Ok(mut f) = std::fs::File::open(file_path) {
                let mut contents = Vec::new();
                if let Ok(_) = f.read_to_end(&mut contents) {
                    self.cache.insert(path.to_owned(), Rc::new(contents));
                }
            }
        }
        self.cache.get(path).map(|v| v.clone())
    }
}
