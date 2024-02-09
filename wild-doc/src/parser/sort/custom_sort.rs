use std::{cmp::Ordering, num::NonZeroU32, sync::Arc};

use wild_doc_script::{CustomSort, SearchResult};

pub struct WdCustomSort {
    pub(super) result: Arc<SearchResult>,
    pub(super) join_name: String,
    pub(super) property: String,
}

impl CustomSort for WdCustomSort {
    fn compare(&self, a: NonZeroU32, b: NonZeroU32) -> std::cmp::Ordering {
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

    fn asc(&self) -> Vec<NonZeroU32> {
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

    fn desc(&self) -> Vec<NonZeroU32> {
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
