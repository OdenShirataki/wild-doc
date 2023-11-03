mod include;
mod value;

use std::{path::PathBuf, sync::Arc};

pub use async_trait::async_trait;
pub use include::IncludeAdaptor;
use parking_lot::Mutex;
pub use value::{Vars, WildDocValue};

pub use anyhow;
pub use serde_json;

use anyhow::Result;

#[async_trait(?Send)]
pub trait WildDocScript {
    fn new(
        include_adaptor: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>,
        cache_dir: PathBuf,
    ) -> Result<Self>
    where
        Self: Sized;
    async fn evaluate_module(&self, file_name: &str, src: &str, stack: &Vars) -> Result<()>;
    async fn eval(&self, code: &str, stack: &Vars) -> Result<Arc<WildDocValue>>;
}
