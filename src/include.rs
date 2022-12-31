use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
};

pub trait IncludeAdaptor {
    fn include<P: AsRef<Path>>(&mut self, path: P) -> &str;
}
pub struct IncludeLocal {
    dir: PathBuf,
    cache: HashMap<PathBuf, String>,
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
    fn include<P: AsRef<Path>>(&mut self, path: P) -> &str {
        let path = path.as_ref().to_path_buf();
        self.cache
            .entry(path.to_owned())
            .or_insert_with_key(|path| {
                let mut file_path = self.dir.clone();
                file_path.push(&path);

                if let Ok(mut f) = std::fs::File::open(file_path) {
                    let mut contents = String::new();
                    let _ = f.read_to_string(&mut contents);
                    contents
                } else {
                    "".to_string()
                }
            })
    }
}
