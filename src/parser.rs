mod process;
mod result;
mod search;
mod update;

use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use deno_runtime::deno_core::{
    self,
    v8::{self, PropertyAttribute},
};
use maybe_xml::{
    scanner::{Scanner, State},
    token::{
        self,
        prop::{Attributes, TagName},
    },
};
use semilattice_database_session::{Session, SessionDatabase};

use crate::{
    anyhow::Result,
    deno::{self, push_stack, Deno},
    xml_util, IncludeAdaptor,
};

type AttributeMap = HashMap<Vec<u8>, Option<String>>;

pub struct Parser<T: IncludeAdaptor> {
    database: Arc<RwLock<SessionDatabase>>,
    sessions: Vec<(Session, bool)>,
    deno: Deno,
    include_adaptor: Arc<Mutex<T>>,
    include_stack: Vec<String>,
}
impl<T: IncludeAdaptor> Parser<T> {
    pub fn new(
        database: Arc<RwLock<SessionDatabase>>,
        include_adaptor: Arc<Mutex<T>>,
        module_cache_dir: PathBuf,
    ) -> Result<Self> {
        Ok(Self {
            deno: Deno::new(&database, &include_adaptor, module_cache_dir)?,
            sessions: vec![],
            database,
            include_adaptor,
            include_stack: vec![],
        })
    }

    pub fn parse_xml(&mut self, input_json: &[u8], xml: &[u8]) -> Result<super::WildDocResult> {
        self.deno.input(input_json)?;
        {}
        let result_body = self.parse(xml)?;
        let result_options = self.deno.eval_json_string(b"wd.result_options");
        Ok(super::WildDocResult {
            body: result_body,
            options_json: result_options,
        })
    }
    fn parse_wd_start_or_empty_tag(
        &mut self,
        name: &[u8],
        attributes: &AttributeMap,
    ) -> Result<Option<Vec<u8>>> {
        match name {
            b"print" => {
                return Ok(
                    if let Some(Some(value)) = attributes.get(b"value".as_ref()) {
                        Some(value.as_bytes().to_vec())
                    } else {
                        None
                    },
                );
            }
            b"include" => {
                return Ok(Some(self.get_include_content(attributes)?));
            }
            b"delete_collection" => {
                self.delete_collection(attributes)?;
            }
            b"session_gc" => {
                self.session_gc(attributes)?;
            }
            _ => {}
        }
        Ok(None)
    }
    fn is_wd_tag(name: &TagName) -> bool {
        if let Some(prefix) = name.namespace_prefix() {
            prefix.as_bytes() == b"wd"
        } else {
            false
        }
    }
    fn output_attrbutes(&mut self, r: &mut Vec<u8>, attributes: Attributes) {
        for attribute in attributes {
            r.push(b' ');
            let name = attribute.name();
            if let Some(prefix) = name.namespace_prefix() {
                match prefix.as_bytes() {
                    b"wd" => {
                        r.append(&mut name.local().to_vec());
                        if let Some(value) = attribute.value() {
                            r.push(b'=');
                            r.push(b'"');
                            r.append(
                                &mut self
                                    .deno
                                    .eval_string(crate::quot_unescape(value.as_bytes()).as_ref())
                                    .as_bytes()
                                    .to_vec(),
                            );
                            r.push(b'"');
                        }
                    }
                    b"wd-attr" => {
                        if name.local().as_bytes() == b"replace" {
                            if let Some(value) = attribute.value() {
                                r.append(
                                    &mut self
                                        .deno
                                        .eval_string(
                                            crate::quot_unescape(value.as_bytes()).as_ref(),
                                        )
                                        .as_bytes()
                                        .to_vec(),
                                );
                            }
                        }
                    }
                    _ => {
                        r.append(&mut attribute.to_vec());
                    }
                }
            } else {
                r.append(&mut attribute.to_vec());
            }
        }
    }

    fn parse_attibutes(&mut self, attributes: Option<Attributes>) -> AttributeMap {
        let mut r = HashMap::new();
        if let Some(attributes) = attributes {
            for attribute in attributes.iter() {
                let name = attribute.name();
                if name.as_bytes().starts_with(b"wd:") {
                    r.insert(
                        name.local().as_bytes().to_vec(),
                        if let Some(value) = attribute.value() {
                            Some(
                                self.deno
                                    .eval_string(crate::quot_unescape(value.as_bytes()).as_ref()),
                            )
                        } else {
                            None
                        },
                    );
                } else {
                    r.insert(
                        name.to_vec(),
                        if let Some(ref value) = attribute.value() {
                            Some(
                                if let Ok(value) = value.to_str() {
                                    value
                                } else {
                                    ""
                                }
                                .to_owned(),
                            )
                        } else {
                            None
                        },
                    );
                }
            }
        }
        r
    }
    pub fn parse(&mut self, xml: &[u8]) -> Result<Vec<u8>> {
        let mut r: Vec<u8> = Vec::new();
        let mut tag_stack = vec![];
        let mut search_map = HashMap::new();
        let mut xml = xml;

        let mut scanner = Scanner::new();

        while let Some(state) = scanner.scan(&xml) {
            match state {
                State::ScannedProcessingInstruction(pos) => {
                    let token_bytes = &xml[..pos];
                    let token = token::borrowed::ProcessingInstruction::from(token_bytes);
                    if token.target().to_str()? == "typescript" {
                        if let Some(i) = token.instructions() {
                            if let Err(e) = self.deno.evaluate_module(
                                if let Some(last) = self.include_stack.last() {
                                    last
                                } else {
                                    ""
                                },
                                i.as_bytes(),
                            ) {
                                return Err(e);
                            }
                        }
                    } else {
                        r.append(&mut token_bytes.to_vec());
                    }
                    xml = &xml[pos..];
                }
                State::ScannedStartTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::StartTag::from(token_bytes);
                    let attributes = token.attributes();
                    let name = token.name();
                    if Self::is_wd_tag(&name) {
                        let attributes = self.parse_attibutes(attributes);
                        if let Some(mut parsed) =
                            self.parse_wd_start_or_empty_tag(name.local().as_bytes(), &attributes)?
                        {
                            r.append(&mut parsed);
                        } else {
                            match name.local().as_bytes() {
                                b"session" => {
                                    self.session(&attributes)?;
                                }
                                b"session_sequence_cursor" => {
                                    self.session_sequence(&attributes)?;
                                }
                                b"sessions" => {
                                    self.sessions(&attributes);
                                }
                                b"re" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let parsed = self.parse(inner_xml)?;
                                    xml = &xml[outer_end..];
                                    r.append(&mut self.parse(&parsed)?);
                                }
                                b"letitgo" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(&mut inner_xml.to_vec());
                                    xml = &xml[outer_end..];
                                }
                                b"update" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    self.update(inner_xml, &attributes)?;
                                    xml = &xml[outer_end..];
                                }
                                b"search" => {
                                    xml = self.search(xml, &attributes, &mut search_map);
                                }
                                b"result" => {
                                    self.result(&attributes, &search_map);
                                }
                                b"collections" => {
                                    self.collections(&attributes);
                                }
                                b"stack" => {
                                    if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
                                        self.deno.execute_script(
                                            "stack.push",
                                            ("wd.stack.push({".to_owned()
                                                + crate::quot_unescape(var.as_bytes()).as_str()
                                                + "});")
                                                .into(),
                                        )?;
                                    }
                                }
                                b"case" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(&mut self.case(&attributes, inner_xml)?);
                                    xml = &xml[outer_end..];
                                }
                                b"if" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(&mut self.r#if(&attributes, inner_xml)?);
                                    xml = &xml[outer_end..];
                                }
                                b"for" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(&mut self.r#for(&attributes, inner_xml)?);
                                    xml = &xml[outer_end..];
                                }
                                b"tag" => {
                                    let (name, mut attr) = self.custom_tag(&attributes);
                                    tag_stack.push(name.clone());
                                    r.push(b'<');
                                    r.append(&mut name.into_bytes());
                                    r.append(&mut attr);
                                    r.push(b'>');
                                }
                                _ => {}
                            }
                        }
                    } else {
                        r.push(b'<');
                        r.append(&mut name.to_vec());
                        if let Some(attributes) = attributes {
                            self.output_attrbutes(&mut r, attributes)
                        }
                        r.push(b'>');
                    }
                }
                State::ScannedEmptyElementTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::EmptyElementTag::from(token_bytes);
                    let name = token.name();
                    if name.as_bytes() == b"wd:tag" {
                        let attributes = self.parse_attibutes(token.attributes());
                        let (name, mut attr) = self.custom_tag(&attributes);
                        r.push(b'<');
                        r.append(&mut name.into_bytes());
                        r.append(&mut attr);
                        r.push(b' ');
                        r.push(b'/');
                        r.push(b'>');
                    } else {
                        if Self::is_wd_tag(&name) {
                            let attributes = self.parse_attibutes(token.attributes());
                            if let Some(mut parsed) = self
                                .parse_wd_start_or_empty_tag(name.local().as_bytes(), &attributes)?
                            {
                                r.append(&mut parsed);
                            }
                        } else {
                            r.push(b'<');
                            r.append(&mut name.to_vec());
                            if let Some(attributes) = token.attributes() {
                                self.output_attrbutes(&mut r, attributes)
                            }
                            r.push(b' ');
                            r.push(b'/');
                            r.push(b'>');
                        }
                    }
                }
                State::ScannedEndTag(pos) => {
                    let token_bytes = &xml[..pos];
                    xml = &xml[pos..];
                    let token = token::borrowed::EndTag::from(token_bytes);
                    let name = token.name();
                    if if let Some(prefix) = name.namespace_prefix() {
                        prefix.as_bytes() == b"wd"
                    } else {
                        false
                    } {
                        match name.local().as_bytes() {
                            b"stack"
                            | b"result"
                            | b"collections"
                            | b"sessions"
                            | b"session_sequence_cursor" => {
                                let _ = self.deno.execute_script(
                                    "stack.pop",
                                    "wd.stack.pop();".to_owned().into(),
                                );
                            }
                            b"session" => {
                                if let Some((ref mut session, clear_on_close)) = self.sessions.pop()
                                {
                                    if clear_on_close {
                                        let _ = self
                                            .database
                                            .clone()
                                            .write()
                                            .unwrap()
                                            .session_clear(session);
                                    }
                                }
                            }
                            b"tag" => {
                                if let Some(name) = tag_stack.pop() {
                                    r.append(&mut b"</".to_vec());
                                    r.append(&mut name.into_bytes());
                                    r.push(b'>');
                                }
                            }
                            _ => {}
                        }
                    } else {
                        r.append(&mut token_bytes.to_vec());
                    }
                }
                State::ScannedCharacters(pos)
                | State::ScannedCdata(pos)
                | State::ScannedComment(pos)
                | State::ScannedDeclaration(pos) => {
                    r.append(&mut xml[..pos].to_vec());
                    xml = &xml[pos..];
                }
                State::ScanningCharacters => {
                    r.append(&mut xml.to_vec());
                    break;
                }
                _ => {}
            }
        }

        Ok(r)
    }

    fn collections(&mut self, attributes: &AttributeMap) {
        let scope = &mut self.deno.js_runtime.handle_scope();

        let obj = v8::Object::new(scope);

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            if var != "" {
                if let (Ok(array), Some(v8str_var)) = (
                    deno_core::serde_v8::to_v8(scope, self.database.read().unwrap().collections()),
                    v8::String::new(scope, &var),
                ) {
                    obj.define_own_property(
                        scope,
                        v8str_var.into(),
                        array.into(),
                        PropertyAttribute::READ_ONLY,
                    );
                }
            }
        }
        push_stack(scope, obj);
    }
    fn session(&mut self, attributes: &AttributeMap) -> io::Result<()> {
        if let Some(Some(session_name)) = attributes.get(b"name".as_ref()) {
            if session_name != "" {
                let clear_on_close =
                    if let Some(Some(col)) = attributes.get(b"clear_on_close".as_ref()) {
                        col == "true"
                    } else {
                        false
                    };

                let expire = if let Some(Some(expire)) = attributes.get(b"expire".as_ref()) {
                    expire
                } else {
                    ""
                };
                let expire = if expire.len() > 0 {
                    expire.parse::<i64>().ok()
                } else {
                    None
                };
                if let Ok(mut session) =
                    Session::new(&self.database.read().unwrap(), session_name, expire)
                {
                    if let Some(Some(cursor)) = attributes.get(b"cursor".as_ref()) {
                        if cursor != "" {
                            if let Ok(cursor) = cursor.parse::<usize>() {
                                session.set_sequence_cursor(cursor)
                            }
                        }
                    }
                    if let Some(Some(initialize)) = attributes.get(b"initialize".as_ref()) {
                        if initialize == "true" {
                            self.database
                                .clone()
                                .read()
                                .unwrap()
                                .session_restart(&mut session, expire)?;
                        }
                    }
                    self.sessions.push((session, clear_on_close));
                }
            }
        }
        Ok(())
    }
    fn sessions(&mut self, attributes: &AttributeMap) {
        let scope = &mut self.deno.js_runtime.handle_scope();

        let obj = v8::Object::new(scope);

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            if var != "" {
                if let (Ok(sessions), Some(v8str_var)) = (
                    self.database.read().unwrap().sessions(),
                    v8::String::new(scope, &var),
                ) {
                    if let Ok(array) = deno_core::serde_v8::to_v8(scope, sessions) {
                        obj.define_own_property(
                            scope,
                            v8str_var.into(),
                            array.into(),
                            PropertyAttribute::READ_ONLY,
                        );
                    }
                }
            }
        }
        push_stack(scope, obj);
    }
    fn session_sequence(&mut self, attributes: &AttributeMap) -> io::Result<()> {
        let mut str_max = if let Some(Some(s)) = attributes.get(b"max".as_ref()) {
            s
        } else {
            ""
        };
        if str_max == "" {
            str_max = "wd:session_sequence_max";
        }

        let mut str_current = if let Some(Some(s)) = attributes.get(b"current".as_ref()) {
            s
        } else {
            ""
        };
        if str_current == "" {
            str_current = "wd:session_sequence_current";
        }

        let scope = &mut self.deno.js_runtime.handle_scope();

        let obj = v8::Object::new(scope);
        if let Some((session, _)) = self.sessions.last() {
            if let Some(cursor) = session.sequence_cursor() {
                if let (Some(v8str_max), Some(v8str_current)) = (
                    v8::String::new(scope, &str_max),
                    v8::String::new(scope, &str_current),
                ) {
                    let max = v8::Integer::new(scope, cursor.max as i32);
                    let current = v8::Integer::new(scope, cursor.current as i32);
                    obj.define_own_property(
                        scope,
                        v8str_max.into(),
                        max.into(),
                        PropertyAttribute::READ_ONLY,
                    );
                    obj.define_own_property(
                        scope,
                        v8str_current.into(),
                        current.into(),
                        PropertyAttribute::READ_ONLY,
                    );
                }
            }
        }

        push_stack(scope, obj);
        Ok(())
    }
    fn session_gc(&mut self, attributes: &AttributeMap) -> io::Result<()> {
        let mut expire = 60 * 60 * 24;
        if let Some(Some(str_expire)) = attributes.get(b"expire".as_ref()) {
            if let Ok(parsed) = str_expire.parse::<i64>() {
                expire = parsed;
            }
        }
        self.database.write().unwrap().session_gc(expire)
    }
    fn delete_collection(&mut self, attributes: &AttributeMap) -> Result<()> {
        if let Some(Some(str_collection)) = attributes.get(b"collection".as_ref()) {
            self.database
                .clone()
                .write()
                .unwrap()
                .delete_collection(str_collection)?;
        }
        Ok(())
    }

    fn custom_tag(&mut self, attributes: &AttributeMap) -> (String, Vec<u8>) {
        let scope = &mut self.deno.js_runtime.handle_scope();
        let mut html_attr = vec![];
        let mut name = "".to_string();
        for (key, value) in attributes {
            if let Some(value) = value {
                if key == b"wd-tag:name" {
                    name = deno::eval_result_string(scope, value.as_bytes());
                } else if key == b"wd-attr:replace" {
                    let attr = deno::eval_result_string(
                        scope,
                        crate::quot_unescape(value.as_bytes()).as_bytes(),
                    );
                    if attr.len() > 0 {
                        html_attr.push(b' ');
                        html_attr.append(&mut attr.into_bytes());
                    }
                } else {
                    html_attr.push(b' ');
                    html_attr.append(&mut key.to_vec());
                    html_attr.push(b'=');
                    html_attr.push(b'"');
                    html_attr.append(
                        &mut value
                            .replace("&", "&amp;")
                            .replace("<", "&lt;")
                            .replace(">", "&gt;")
                            .into_bytes(),
                    );
                    html_attr.push(b'"');
                }
            }
        }

        (name, html_attr)
    }
}
