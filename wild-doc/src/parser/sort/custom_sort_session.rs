use std::{cmp::Ordering, num::NonZeroI64, sync::Arc};

use semilattice_database_session::{SessionCustomOrder, SessionSearchResult};

pub struct WdCustomSortSession {
    pub(super) result: Arc<SessionSearchResult>,
    pub(super) join_name: String,
    pub(super) property: String,
}

impl SessionCustomOrder for WdCustomSortSession {
    fn compare(&self, a: NonZeroI64, b: NonZeroI64) -> std::cmp::Ordering {
        if let Some(join) = self.result.as_ref().join().get(&self.join_name) {
            match self.property.as_str() {
                "len" => {
                    if let (Some(a), Some(b)) = (join.get(&a), join.get(&b)) {
                        return a.rows().len().cmp(&b.rows().len());
                    }
                }
                _ => {}
            }
        }
        Ordering::Equal
    }

    fn asc(&self) -> Vec<NonZeroI64> {
        if let Some(join) = self.result.join().get(&self.join_name) {
            match self.property.as_str() {
                "len" => {
                    let mut sorted: Vec<_> = self.result.rows().into_iter().cloned().collect();
                    sorted.sort_by(|a, b| {
                        if let (Some(a), Some(b)) = (join.get(a), join.get(b)) {
                            a.rows().len().cmp(&b.rows().len())
                        } else {
                            Ordering::Equal
                        }
                    });
                    return sorted;
                }
                _ => {}
            }
        }
        vec![]
    }

    fn desc(&self) -> Vec<NonZeroI64> {
        if let Some(join) = self.result.join().get(&self.join_name) {
            match self.property.as_str() {
                "len" => {
                    let mut sorted: Vec<_> = self.result.rows().into_iter().cloned().collect();
                    sorted.sort_by(|a, b| {
                        if let (Some(a), Some(b)) = (join.get(a), join.get(b)) {
                            b.rows().len().cmp(&a.rows().len())
                        } else {
                            Ordering::Equal
                        }
                    });
                    return sorted;
                }
                _ => {}
            }
        }
        vec![]
    }
}
