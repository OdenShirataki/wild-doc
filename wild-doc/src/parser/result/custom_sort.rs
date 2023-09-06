use std::{
    cmp::Ordering,
    sync::{Arc, RwLock},
};

use semilattice_database_session::{search::SearchResult, CustomSort};

pub struct WdCustomSort {
    pub(super) result: Arc<RwLock<Option<SearchResult>>>,
    pub(super) join_name: String,
    pub(super) property: String,
}

impl CustomSort for WdCustomSort {
    fn compare(&self, a: u32, b: u32) -> std::cmp::Ordering {
        if let Some(result) = self.result.read().unwrap().as_ref() {
            if let Some(join) = result.join().get(&self.join_name) {
                match self.property.as_str() {
                    "len" => {
                        if let (Some(a), Some(b)) = (join.get(&a), join.get(&b)) {
                            return a.rows().len().cmp(&b.rows().len());
                        }
                    }
                    _ => {}
                }
            }
        }
        Ordering::Equal
    }
    fn asc(&self) -> Vec<u32> {
        if let Some(result) = self.result.read().unwrap().as_ref() {
            if let Some(join) = result.join().get(&self.join_name) {
                match self.property.as_str() {
                    "len" => {
                        let mut sorted = result.rows().iter().map(|&x| x).collect::<Vec<u32>>();
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
        }
        vec![]
    }
    fn desc(&self) -> Vec<u32> {
        if let Some(result) = self.result.read().unwrap().as_ref() {
            if let Some(join) = result.join().get(&self.join_name) {
                match self.property.as_str() {
                    "len" => {
                        let mut sorted = result.rows().iter().map(|&x| x).collect::<Vec<u32>>();
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
        }
        vec![]
    }
}
