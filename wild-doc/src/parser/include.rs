use std::{path::Path, sync::Arc};

use anyhow::Result;
use wild_doc_script::Vars;

use crate::r#const::*;

use super::Parser;

impl Parser {
    pub(super) async fn get_include_content(
        &mut self,
        attr: Vars,
        with_parse: bool,
    ) -> Result<Vec<u8>> {
        if let Some(src) = attr.get(&*SRC) {
            let src = src.as_string();
            let (xml, filename) = self
                .include_adaptor
                .lock()
                .include(Path::new(src.as_str()))
                .map_or_else(
                    || {
                        let mut r = (None, Arc::clone(&*_BLANK));
                        if let Some(substitute) = attr.get(&*SUBSTITUTE) {
                            let substitute = substitute.as_string();
                            if let Some(xml) = self
                                .include_adaptor
                                .lock()
                                .include(Path::new(substitute.as_str()))
                            {
                                r = (Some(xml), substitute);
                            }
                        }
                        r
                    },
                    |xml| (Some(xml), src),
                );
            if let Some(xml) = xml {
                if xml.len() > 0 {
                    return Ok(if with_parse {
                        self.include_stack.push(filename.into());
                        let mut pos = 0;
                        let r = self.parse(xml.as_slice(), &mut pos).await?;
                        self.include_stack.pop();
                        r
                    } else {
                        xml.as_ref().clone()
                    });
                }
            }
        }
        Ok(b"".to_vec())
    }
}
