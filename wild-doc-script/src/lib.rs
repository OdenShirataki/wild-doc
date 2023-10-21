mod include;
mod state;
mod value;

use std::sync::Arc;

pub use async_trait::async_trait;
pub use include::IncludeAdaptor;
pub use state::WildDocState;
pub use value::{Vars, VarsStack, WildDocValue};

pub use anyhow;
pub use serde_json;

use anyhow::Result;

#[async_trait(?Send)]
pub trait WildDocScript {
    fn new(state: Arc<WildDocState>) -> Result<Self>
    where
        Self: Sized;
    async fn evaluate_module(&mut self, file_name: &str, src: &[u8]) -> Result<()>;
    async fn eval(&mut self, code: &[u8]) -> Result<WildDocValue>;
}
