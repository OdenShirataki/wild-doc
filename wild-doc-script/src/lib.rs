mod include;
mod state;
mod value;

pub use include::IncludeAdaptor;
pub use state::WildDocState;
pub use value::{Vars, VarsStack, WildDocValue};

pub use anyhow;
pub use serde_json;

use anyhow::Result;

pub trait WildDocScript {
    fn new(state: WildDocState) -> Result<Self>
    where
        Self: Sized;
    fn evaluate_module(&self, file_name: &str, src: &[u8]) -> Result<()>;
    fn eval(&self, code: &[u8]) -> Result<WildDocValue>;
}
