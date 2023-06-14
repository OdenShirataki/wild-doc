mod process;
mod result;
mod search;
mod update;

use std::{
    borrow::Cow,
    collections::HashMap,
    io,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, RwLock},
};

use deno_runtime::deno_core::{
    self, serde_json,
    v8::{self, PropertyAttribute},
};
use maybe_xml::{
    scanner::{Scanner, State},
    token::{
        self,
        prop::{Attribute, Attributes, TagName},
    },
};
use semilattice_database_session::{Activity, CollectionRow, Session, SessionDatabase, Uuid};

use crate::{
    anyhow::Result,
    deno::{self, push_stack, Deno},
    xml_util, IncludeAdaptor,
};

#[derive(Debug)]
pub struct WildDocValue {
    pub(crate) value: serde_json::Value,
}
impl WildDocValue {
    pub fn new(value: serde_json::Value) -> Self {
        Self { value }
    }
    pub fn to_str<'a>(&'a self) -> Cow<'a, str> {
        if let Some(s) = self.value.as_str() {
            Cow::Borrowed(s)
        } else {
            Cow::Owned(self.value.to_string())
        }
    }
}
type AttributeMap = HashMap<Vec<u8>, Option<Rc<WildDocValue>>>;
pub type VarsStack = Vec<HashMap<Vec<u8>, Rc<WildDocValue>>>;

pub struct Parser<T: IncludeAdaptor> {
    database: Arc<RwLock<SessionDatabase>>,
    sessions: Vec<(Session, bool)>,
    stack: Arc<RwLock<VarsStack>>,
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
        let stack = Arc::new(RwLock::new(vec![]));
        Ok(Self {
            deno: Deno::new(&database, &include_adaptor, module_cache_dir, &stack)?,
            sessions: vec![],
            stack,
            database,
            include_adaptor,
            include_stack: vec![],
        })
    }

    pub fn parse_xml(&mut self, input_json: &[u8], xml: &[u8]) -> Result<super::WildDocResult> {
        let mut json: HashMap<Vec<u8>, Rc<WildDocValue>> = HashMap::new();
        json.insert(
            b"input".to_vec(),
            Rc::new(WildDocValue::new(
                if let Ok(json) = serde_json::from_slice(input_json) {
                    json
                } else {
                    serde_json::json!([])
                },
            )),
        );
        self.stack.write().unwrap().push(json);
        let result_body = self.parse(xml)?;
        self.stack.write().unwrap().pop();
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
                        Some(value.to_str().into_owned().into_bytes())
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
    fn search_stack(&self, key: &[u8]) -> Option<Rc<WildDocValue>> {
        for stack in self.stack.read().unwrap().iter().rev() {
            if let Some(v) = stack.get(key) {
                return Some(v.clone());
            }
        }
        None
    }
    fn parse_attibute_value(&self, attribute: &Attribute) -> Option<Rc<WildDocValue>> {
        let mut value = None;
        if let Some(attribute_value) = attribute.value() {
            let mut splited = attribute_value.as_bytes().split(|c| *c == b'.');
            if let Some(root) = splited.next() {
                if let Some(root) = self.search_stack(root) {
                    let mut next_value = &root.value;
                    while {
                        if let Some(next) = splited.next() {
                            match next_value {
                                serde_json::Value::Object(map) => {
                                    let mut ret = false;
                                    if let Some(v) =
                                        map.get(unsafe { std::str::from_utf8_unchecked(next) })
                                    {
                                        next_value = v;
                                        ret = true;
                                    }
                                    ret
                                }
                                serde_json::Value::Array(map) => {
                                    let mut ret = false;
                                    if let Ok(index) = std::str::from_utf8(next) {
                                        if let Ok(index) = index.parse::<usize>() {
                                            if let Some(v) = map.get(index) {
                                                next_value = v;
                                                ret = true;
                                            }
                                        }
                                    }
                                    ret
                                }
                                _ => false,
                            }
                        } else {
                            value = Some(Rc::new(WildDocValue::new(next_value.clone())));
                            false
                        }
                    } {}
                }
            }
        }
        value
    }
    fn attribute_value_script(&mut self, attribute: &Attribute) -> Option<Rc<WildDocValue>> {
        if let Some(value) = attribute.value() {
            let source = "(".to_owned() + crate::quot_unescape(value.as_bytes()).as_str() + ")";
            if let Some(v) = self.deno.eval_json_value(source.as_ref()) {
                Some(Rc::new(WildDocValue::new(v)))
            } else {
                None
            }
        } else {
            None
        }
    }
    fn output_attrbute_value(r: &mut Vec<u8>, val: &[u8]) {
        r.push(b'=');
        r.push(b'"');
        r.append(&mut val.to_vec());
        r.push(b'"');
    }
    fn output_attrbutes(&mut self, r: &mut Vec<u8>, attributes: Attributes) {
        for attribute in attributes {
            r.push(b' ');
            let name = attribute.name();
            let name_bytes = name.as_bytes();
            if name_bytes.ends_with(b":var") {
                if let Some(prefix) = name.namespace_prefix() {
                    r.append(&mut prefix.as_bytes().to_vec());
                    if let Some(value) = self.parse_attibute_value(&attribute) {
                        Self::output_attrbute_value(r, &mut value.to_str().as_bytes());
                    }
                }
            } else if name_bytes.ends_with(b":script") {
                if let Some(prefix) = name.namespace_prefix() {
                    r.append(&mut prefix.as_bytes().to_vec());
                    if let Some(value) = self.attribute_value_script(&attribute) {
                        Self::output_attrbute_value(r, &mut value.to_str().as_bytes());
                    }
                }
            } else if name_bytes == b"wd-attr:replace" {
                if name.local().as_bytes() == b"replace" {
                    if let Some(value) = attribute.value() {
                        r.append(
                            &mut self
                                .deno
                                .eval_string(crate::quot_unescape(value.as_bytes()).as_ref())
                                .as_bytes()
                                .to_vec(),
                        );
                    }
                }
            } else {
                r.append(&mut attribute.to_vec());
            }
        }
    }

    fn parse_attibutes(&mut self, attributes: Option<Attributes>) -> AttributeMap {
        let mut r: AttributeMap = HashMap::new();
        if let Some(attributes) = attributes {
            for attribute in attributes.iter() {
                let name = attribute.name();
                let name_bytes = name.as_bytes();
                if name_bytes.ends_with(b":var") {
                    if let Some(prefix) = name.namespace_prefix() {
                        r.insert(
                            prefix.as_bytes().to_vec(),
                            self.parse_attibute_value(&attribute),
                        );
                    }
                } else if name_bytes.ends_with(b":script") {
                    if let Some(prefix) = name.namespace_prefix() {
                        r.insert(
                            prefix.as_bytes().to_vec(),
                            self.attribute_value_script(&attribute),
                        );
                    }
                } else {
                    r.insert(
                        name.to_vec(),
                        if let Some(ref value) = attribute.value() {
                            let value = crate::quot_unescape(value.as_bytes());
                            if let Ok(json_value) = serde_json::from_str(value.as_str()) {
                                Some(Rc::new(WildDocValue::new(json_value)))
                            } else {
                                Some(Rc::new(WildDocValue::new(
                                    serde_json::json!(value.as_str()),
                                )))
                            }
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
                    match token.target().to_str()? {
                        "typescript" | "script" => {
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
                        }
                        _ => {
                            r.append(&mut token_bytes.to_vec());
                        }
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
                                    self.result(&attributes, &search_map)?;
                                }
                                b"collections" => {
                                    self.collections(&attributes);
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

                                b"def" => {
                                    self.def(&attributes);
                                }
                                b"row" => {
                                    self.row(&attributes);
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
                            b"def" | b"result" => {
                                self.stack.write().unwrap().pop();
                            }
                            b"collections" | b"sessions" | b"session_sequence_cursor" => {
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

    fn def(&mut self, attributes: &AttributeMap) {
        let mut json: HashMap<Vec<u8>, Rc<WildDocValue>> = HashMap::new();
        for (key, v) in attributes {
            if let Some(v) = v {
                json.insert(key.to_vec(), v.clone());
            }
        }
        self.stack.write().unwrap().push(json);
    }
    fn row(&mut self, attributes: &AttributeMap) {
        let mut json: HashMap<Vec<u8>, Rc<WildDocValue>> = HashMap::new();

        if let (Some(Some(collection_id)), Some(Some(row)), Some(Some(var))) = (
            attributes.get(b"collection_id".as_ref()),
            attributes.get(b"row".as_ref()),
            attributes.get(b"var".as_ref()),
        ) {
            let var = var.to_str();
            if var != "" {
                let mut json_inner = serde_json::Map::new();
                if let Some(collection_id) = collection_id.value.as_i64() {
                    let collection_id = collection_id as i32;
                    let mut session_maybe_has_collection = None;
                    for i in (0..self.sessions.len()).rev() {
                        if let Some(temporary_collection) =
                            self.sessions[i].0.temporary_collection(collection_id)
                        {
                            session_maybe_has_collection = Some(temporary_collection);
                            break;
                        }
                    }
                    if let Some(row) = row.value.as_i64() {
                        let mut json_field = serde_json::Map::new();
                        if let Some(temporary_collection) = session_maybe_has_collection {
                            if let Some(entity) = temporary_collection.get(&row) {
                                json_inner.insert(
                                    "uuid".to_owned(),
                                    serde_json::json!(Uuid::from_u128(entity.uuid()).to_string()),
                                );
                                json_inner.insert(
                                    "activity".to_owned(),
                                    serde_json::json!(entity.activity() == Activity::Active),
                                );
                                json_inner.insert(
                                    "term_begin".to_owned(),
                                    serde_json::json!(entity.term_begin()),
                                );
                                json_inner.insert(
                                    "term_begin".to_owned(),
                                    serde_json::json!(entity.term_end()),
                                );
                                json_inner.insert(
                                    "depends".to_owned(),
                                    serde_json::json!(entity.depends()),
                                );
                                if let Some(Some(field)) = attributes.get(b"field".as_ref()) {
                                    let fields = entity.fields();
                                    for field_name in field.to_str().split(",") {
                                        if let Some(value) = fields.get(field_name) {
                                            if let Ok(value) = std::str::from_utf8(value) {
                                                json_field.insert(
                                                    field_name.to_owned(),
                                                    serde_json::json!(value),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            if row > 0 {
                                if let Some(collection) =
                                    self.database.read().unwrap().collection(collection_id)
                                {
                                    let row = row as u32;

                                    json_inner.insert(
                                        "uuid".to_owned(),
                                        serde_json::json!(
                                            Uuid::from_u128(collection.uuid(row)).to_string()
                                        ),
                                    );
                                    json_inner.insert(
                                        "activity".to_owned(),
                                        serde_json::json!(
                                            collection.activity(row) == Activity::Active
                                        ),
                                    );
                                    json_inner.insert(
                                        "term_begin".to_owned(),
                                        serde_json::json!(collection.term_begin(row)),
                                    );
                                    json_inner.insert(
                                        "term_end".to_owned(),
                                        serde_json::json!(collection.term_end(row)),
                                    );
                                    json_inner.insert(
                                        "depends".to_owned(),
                                        serde_json::json!(serde_json::json!(self
                                            .database
                                            .read()
                                            .unwrap()
                                            .relation()
                                            .depends(
                                                None,
                                                &CollectionRow::new(collection_id, row)
                                            ))),
                                    );

                                    if let Some(Some(field)) = attributes.get(b"field".as_ref()) {
                                        for field_name in field.to_str().split(",") {
                                            let field = collection.field_bytes(row, field_name);
                                            if let Ok(field) = std::str::from_utf8(field) {
                                                json_field.insert(
                                                    field_name.to_owned(),
                                                    serde_json::json!(field),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        json_inner
                            .insert("field".to_owned(), serde_json::Value::Object(json_field));
                    }
                }
                json.insert(
                    var.as_bytes().to_vec(),
                    Rc::new(WildDocValue::new(serde_json::Value::Object(json_inner))),
                );
            }
        }
        self.stack.write().unwrap().push(json);
    }
    fn collections(&mut self, attributes: &AttributeMap) {
        let scope = &mut self.deno.js_runtime.handle_scope();

        let obj = v8::Object::new(scope);

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            let var = var.to_str();
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
            let session_name = session_name.to_str();
            if session_name != "" {
                let clear_on_close =
                    if let Some(Some(col)) = attributes.get(b"clear_on_close".as_ref()) {
                        col.to_str() == "true"
                    } else {
                        false
                    };

                let expire = if let Some(Some(expire)) = attributes.get(b"expire".as_ref()) {
                    expire.to_str()
                } else {
                    "".into()
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
                        let cursor = cursor.to_str();
                        if cursor != "" {
                            if let Ok(cursor) = cursor.parse::<usize>() {
                                session.set_sequence_cursor(cursor)
                            }
                        }
                    }
                    if let Some(Some(initialize)) = attributes.get(b"initialize".as_ref()) {
                        let initialize = initialize.to_str();
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
            let var = var.to_str();
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
            s.to_str()
        } else {
            "".into()
        };
        if str_max == "" {
            str_max = "wd:session_sequence_max".into();
        }

        let mut str_current = if let Some(Some(s)) = attributes.get(b"current".as_ref()) {
            s.to_str()
        } else {
            "".into()
        };
        if str_current == "" {
            str_current = "wd:session_sequence_current".into();
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
            if let Ok(parsed) = str_expire.to_str().parse::<i64>() {
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
                .delete_collection(str_collection.to_str().as_ref())?;
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
                    //TODO :varなのかscriptなのか
                    name = deno::eval_result_string(scope, value.to_str().as_bytes());
                } else if key == b"wd-attr:replace" {
                    let attr = deno::eval_result_string(
                        scope,
                        crate::quot_unescape(value.to_str().as_bytes()).as_bytes(),
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
                            .to_str()
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
