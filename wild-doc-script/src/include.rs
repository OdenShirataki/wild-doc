use std::{path::Path, sync::Arc};

pub trait IncludeAdaptor {
    fn include(&mut self, path: &Path) -> Option<Arc<Vec<u8>>>;
}
