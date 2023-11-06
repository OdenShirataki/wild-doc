use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use crate::{Vars, WildDocValue};

#[derive(Clone)]
pub struct Stack {
    vars: Vec<Vars>,
}

impl Stack {
    pub fn new(inital: Option<Vars>) -> Self {
        Self {
            vars: inital.map_or(vec![], |v| [v].into()),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Arc<WildDocValue>> {
        for vars in self.vars.iter().rev() {
            if let Some(vars) = vars.get(key) {
                return Some(vars);
            }
        }
        None
    }
}

impl Deref for Stack {
    type Target = Vec<Vars>;

    fn deref(&self) -> &Self::Target {
        &self.vars
    }
}

impl DerefMut for Stack {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.vars
    }
}
