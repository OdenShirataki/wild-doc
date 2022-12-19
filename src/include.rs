use std::{collections::HashMap, io::Read};

pub trait IncludeAdaptor {
    fn include(&mut self, path: &str) -> &str;
}
pub struct IncludeLocal {
    dir: String,
    cache: HashMap<String, String>,
}
impl IncludeLocal {
    pub fn new(dir: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            cache: HashMap::default(),
        }
    }
}
impl IncludeAdaptor for IncludeLocal {
    fn include(&mut self, path: &str) -> &str {
        self.cache.entry(path.to_owned()).or_insert_with_key(|path| {
            if let Ok(mut f) = std::fs::File::open(&(self.dir.clone() + path)) {
                let mut contents = String::new();
                let _ = f.read_to_string(&mut contents);
                contents
            } else {
                "".to_string()
            }
        })
    }
}
