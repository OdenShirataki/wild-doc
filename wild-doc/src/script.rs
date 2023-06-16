use deno_runtime::deno_core::serde_json;

use crate::anyhow::Result;

pub trait Script {
    fn evaluate_module(&mut self, file_name: &str, src: &[u8]) -> Result<()>;
    fn eval(&mut self, code: &[u8]) -> Option<serde_json::Value>;
}
