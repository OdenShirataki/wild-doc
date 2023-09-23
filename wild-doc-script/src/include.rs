use std::{path::PathBuf, sync::Arc};

pub trait IncludeAdaptor {
    fn include(&mut self, path: PathBuf) -> Option<Arc<Vec<u8>>>;
}
