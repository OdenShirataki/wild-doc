mod include;
mod stack;
mod value;

use std::{path::PathBuf, sync::Arc};

pub use anyhow;
pub use async_trait::async_trait;
pub use include::IncludeAdaptor;
pub use serde_json;
pub use stack::Stack;
pub use value::{SessionSearchResult, Vars, WildDocValue};

pub use semilattice_database_session::{
    search, Activity, CollectionRow, Condition, CustomOrderKey, CustomSort, DataOption, Depends,
    FieldName, Order, Pend, SearchResult, Session, SessionCustomOrder, SessionDatabase,
    SessionOrder, SessionOrderKey, SessionRecord, Term, Uuid,
};

use anyhow::Result;
use parking_lot::Mutex;

#[async_trait(?Send)]
pub trait WildDocScript<I: IncludeAdaptor + Send> {
    fn new(include_adaptor: Arc<Mutex<I>>, cache_dir: PathBuf, stack: &Stack) -> Result<Self>
    where
        Self: Sized;
    async fn evaluate_module(&mut self, file_name: &str, src: &str, stack: &Stack) -> Result<()>;
    async fn eval(&mut self, code: &str, stack: &Stack) -> Result<WildDocValue>;
}
