mod include;
mod stack;
mod value;

use std::{path::PathBuf, sync::Arc};

pub use async_trait::async_trait;
pub use include::IncludeAdaptor;
pub use stack::Stack;
pub use value::{Vars, WildDocValue};

pub use anyhow;
use parking_lot::Mutex;
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
    async fn evaluate_module(&mut self, file_name: &str, src: &str, stack: &Stack) -> Result<()>;
    async fn eval(&mut self, code: &str, stack: &Stack) -> Result<WildDocValue>;
}
