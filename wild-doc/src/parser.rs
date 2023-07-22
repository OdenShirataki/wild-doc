mod process;
mod result;
mod search;
mod update;

use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex, RwLock},
};

use maybe_xml::{
    scanner::{Scanner, State},
    token::{
        self,
        prop::{Attributes, TagName},
    },
};
use semilattice_database_session::{Activity, CollectionRow, Session, SessionDatabase, Uuid};
use serde_json::Value;
use wild_doc_script::{Vars, WildDocScript, WildDocState, WildDocValue};

use crate::{anyhow::Result, xml_util};

type AttributeMap = HashMap<Vec<u8>, Option<Arc<WildDocValue>>>;

struct SessionState {
    session: Session,
    commit_on_close: bool,
    clear_on_close: bool,
}
pub struct Parser {
    database: Arc<RwLock<SessionDatabase>>,
    sessions: Vec<SessionState>,
    scripts: Arc<HashMap<String, Arc<Mutex<dyn WildDocScript>>>>,
    state: WildDocState,
    include_stack: Vec<String>,
}
impl Parser {
    pub fn new(
        database: Arc<RwLock<SessionDatabase>>,
        scripts: Arc<HashMap<String, Arc<Mutex<dyn WildDocScript>>>>,
        state: WildDocState,
    ) -> Result<Self> {
        Ok(Self {
            scripts,
            sessions: vec![],
            database,
            state,
            include_stack: vec![],
        })
    }

    pub(crate) fn register_global(&mut self, name: &str, value: &serde_json::Value) {
        if let Some(stack) = self.state.stack().write().unwrap().get(0) {
            if let Some(global) = stack.get(b"global".as_ref()) {
                if let Ok(mut global) = global.write() {
                    let mut json: &mut Value = &mut global;
                    let splited = name.split('.');
                    for s in splited {
                        if !json[s].is_object() {
                            json[s] = serde_json::json!({});
                        }
                        json = &mut json[s];
                    }
                    *json = value.clone();
                }
            }
        }
    }
    fn parse_wd_start_or_empty_tag(
        &mut self,
        name: &[u8],
        attributes: &Option<Attributes<'_>>,
    ) -> Result<Option<Vec<u8>>> {
        match name {
            b"print" => {
                let attributes = self.parse_attibutes(attributes);
                return Ok(
                    if let Some(Some(value)) = attributes.get(b"value".as_ref()) {
                        Some(value.to_str().into_owned().into_bytes())
                    } else {
                        None
                    },
                );
            }
            b"global" => {
                let attributes = self.parse_attibutes(attributes);
                if let (Some(Some(var)), Some(Some(value))) = (
                    attributes.get(b"var".as_ref()),
                    attributes.get(b"value".as_ref()),
                ) {
                    self.register_global(var.to_str().as_ref(), value.value());
                }
            }
            b"print_escape_html" => {
                let attributes = self.parse_attibutes(attributes);
                return Ok(
                    if let Some(Some(value)) = attributes.get(b"value".as_ref()) {
                        Some(xml_util::escape_html(&value.to_str()).into_bytes())
                    } else {
                        None
                    },
                );
            }
            b"include" => {
                let attributes = self.parse_attibutes(attributes);
                return Ok(Some(self.get_include_content(attributes)?));
            }
            b"delete_collection" => {
                let attributes = self.parse_attibutes(attributes);
                self.delete_collection(attributes)?;
            }
            b"session_gc" => {
                let attributes = self.parse_attibutes(attributes);
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
    fn attribute_script<'a>(&mut self, script: &str, value: &[u8]) -> Option<WildDocValue> {
        if let Some(script) = self.scripts.get(script) {
            if let Ok(v) = script
                .lock()
                .unwrap()
                .eval(xml_util::quot_unescape(value).as_bytes())
            {
                Some(WildDocValue::new(v))
            } else {
                None
            }
        } else {
            None
        }
    }
    fn output_attribute_value(r: &mut Vec<u8>, val: &[u8]) {
        r.push(b'=');
        r.push(b'"');
        r.append(&mut val.to_vec());
        r.push(b'"');
    }

    fn attibute_var_or_script<'a>(
        &'a mut self,
        name: &'a [u8],
        value: &[u8],
    ) -> (&[u8], Option<WildDocValue>) {
        for key in self.scripts.keys() {
            if name.ends_with((":".to_owned() + key.as_str()).as_bytes()) {
                return (
                    &name[..name.len() - (key.len() + 1)],
                    self.attribute_script(key.to_owned().as_str(), value),
                );
            }
        }
        (name, None)
    }
    fn output_attributes(&mut self, r: &mut Vec<u8>, attributes: Attributes) {
        for attribute in attributes {
            let name = attribute.name();
            if let Some(value) = attribute.value() {
                let (new_name, new_value) =
                    self.attibute_var_or_script(name.as_bytes(), value.as_bytes());
                if new_name == b"wd-attr:replace" {
                    if let Some(value) = new_value {
                        r.push(b' ');
                        r.append(&mut value.to_str().as_bytes().to_vec());
                    }
                } else {
                    r.push(b' ');
                    r.append(&mut new_name.to_vec());
                    if let Some(value) = new_value {
                        Self::output_attribute_value(
                            r,
                            xml_util::escape_html(&value.to_str()).as_bytes(),
                        );
                    } else {
                        Self::output_attribute_value(r, value.as_bytes());
                    }
                }
            } else {
                r.append(&mut attribute.to_vec());
            };
        }
    }

    fn parse_attibutes(&mut self, attributes: &Option<Attributes>) -> AttributeMap {
        let mut r: AttributeMap = HashMap::new();
        if let Some(attributes) = attributes {
            for attribute in attributes.iter() {
                if let Some(value) = attribute.value() {
                    if let (prefix, Some(value)) =
                        self.attibute_var_or_script(attribute.name().as_bytes(), value.as_bytes())
                    {
                        r.insert(prefix.to_vec(), Some(Arc::new(value)));
                    } else {
                        r.insert(attribute.name().to_vec(), {
                            let value = xml_util::quot_unescape(value.as_bytes());
                            if let Ok(json_value) = serde_json::from_str(value.as_str()) {
                                Some(Arc::new(WildDocValue::new(json_value)))
                            } else {
                                Some(Arc::new(WildDocValue::new(serde_json::json!(
                                    value.as_str()
                                ))))
                            }
                        });
                    }
                } else {
                    r.insert(attribute.name().to_vec(), None);
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
                    let target = token.target();
                    if let Some(script) = self.scripts.get(target.to_str()?) {
                        if let Some(i) = token.instructions() {
                            if let Err(e) = script.lock().unwrap().evaluate_module(
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
                    let name = token.name();
                    if Self::is_wd_tag(&name) {
                        if let Some(mut parsed) = self.parse_wd_start_or_empty_tag(
                            name.local().as_bytes(),
                            &token.attributes(),
                        )? {
                            r.append(&mut parsed);
                        } else {
                            match name.local().as_bytes() {
                                b"session" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.session(attributes)?;
                                }
                                b"session_sequence_cursor" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.session_sequence(attributes)?;
                                }
                                b"sessions" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.sessions(&attributes);
                                }
                                b"re" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    let parsed = self.parse(inner_xml)?;
                                    xml = &xml[outer_end..];
                                    r.append(&mut self.parse(&parsed)?);
                                }
                                b"comment" => {
                                    let (_, outer_end) = xml_util::inner(xml);
                                    xml = &xml[outer_end..];
                                }
                                b"letitgo" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(&mut inner_xml.to_vec());
                                    xml = &xml[outer_end..];
                                }
                                b"update" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    self.update(inner_xml, &attributes)?;
                                    xml = &xml[outer_end..];
                                }
                                b"search" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    xml = self.search(xml, &attributes, &mut search_map);
                                }
                                b"result" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.result(&attributes, &search_map)?;
                                }
                                b"collections" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.collections(attributes);
                                }
                                b"case" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(&mut self.case(attributes, inner_xml)?);
                                    xml = &xml[outer_end..];
                                }
                                b"if" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(&mut self.r#if(attributes, inner_xml)?);
                                    xml = &xml[outer_end..];
                                }
                                b"for" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(&mut self.r#for(attributes, inner_xml)?);
                                    xml = &xml[outer_end..];
                                }
                                b"while" => {
                                    let (inner_xml, outer_end) = xml_util::inner(xml);
                                    r.append(&mut self.r#while(token.attributes(), inner_xml)?);
                                    xml = &xml[outer_end..];
                                }
                                b"tag" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    let (name, mut attr) = self.custom_tag(attributes);
                                    tag_stack.push(name.clone());
                                    r.push(b'<');
                                    r.append(&mut name.into_bytes());
                                    r.append(&mut attr);
                                    r.push(b'>');
                                }
                                b"local" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.local(attributes);
                                }
                                b"row" => {
                                    let attributes = self.parse_attibutes(&token.attributes());
                                    self.row(attributes);
                                }
                                _ => {}
                            }
                        }
                    } else {
                        r.push(b'<');
                        r.append(&mut name.to_vec());
                        if let Some(attributes) = token.attributes() {
                            self.output_attributes(&mut r, attributes)
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
                        let attributes = self.parse_attibutes(&token.attributes());
                        let (name, mut attr) = self.custom_tag(attributes);
                        r.push(b'<');
                        r.append(&mut name.into_bytes());
                        r.append(&mut attr);
                        r.push(b' ');
                        r.push(b'/');
                        r.push(b'>');
                    } else {
                        if Self::is_wd_tag(&name) {
                            if let Some(mut parsed) = self.parse_wd_start_or_empty_tag(
                                name.local().as_bytes(),
                                &token.attributes(),
                            )? {
                                r.append(&mut parsed);
                            }
                        } else {
                            r.push(b'<');
                            r.append(&mut name.to_vec());
                            if let Some(attributes) = token.attributes() {
                                self.output_attributes(&mut r, attributes)
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
                            b"local"
                            | b"result"
                            | b"collections"
                            | b"sessions"
                            | b"session_sequence_cursor" => {
                                self.state.stack().write().unwrap().pop();
                            }
                            b"session" => {
                                if let Some(ref mut session_state) = self.sessions.pop() {
                                    if session_state.commit_on_close {
                                        self.database
                                            .write()
                                            .unwrap()
                                            .commit(&mut session_state.session)?;
                                    } else if session_state.clear_on_close {
                                        let _ = self
                                            .database
                                            .clone()
                                            .write()
                                            .unwrap()
                                            .session_clear(&mut session_state.session);
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

    fn local(&mut self, attributes: AttributeMap) {
        let mut json: Vars = HashMap::new();
        for (key, v) in attributes {
            if let Some(v) = v {
                json.insert(key.to_vec(), Arc::new(RwLock::new(v.as_ref().clone())));
            }
        }
        self.state.stack().write().unwrap().push(json);
    }
    fn row(&mut self, attributes: AttributeMap) {
        let mut json = HashMap::new();

        if let (Some(Some(collection_id)), Some(Some(row)), Some(Some(var))) = (
            attributes.get(b"collection_id".as_ref()),
            attributes.get(b"row".as_ref()),
            attributes.get(b"var".as_ref()),
        ) {
            let var = var.to_str();
            if var != "" {
                let mut json_inner = serde_json::Map::new();
                if let Some(collection_id) = collection_id.value().as_i64() {
                    let collection_id = collection_id as i32;
                    let mut session_maybe_has_collection = None;
                    for i in (0..self.sessions.len()).rev() {
                        if let Some(temporary_collection) =
                            self.sessions[i].session.temporary_collection(collection_id)
                        {
                            session_maybe_has_collection = Some(temporary_collection);
                            break;
                        }
                    }
                    if let Some(row) = row.value().as_i64() {
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
                                            .read()
                                            .unwrap()
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
                    Arc::new(RwLock::new(WildDocValue::new(serde_json::Value::Object(
                        json_inner,
                    )))),
                );
            }
        }
        self.state.stack().write().unwrap().push(json);
    }
    fn collections(&mut self, attributes: AttributeMap) {
        let mut json = HashMap::new();

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            let var = var.to_str();
            if var != "" {
                let collections = self.database.read().unwrap().collections();
                json.insert(
                    var.to_string().as_bytes().to_vec(),
                    Arc::new(RwLock::new(WildDocValue::new(serde_json::json!(
                        collections
                    )))),
                );
            }
        }
        self.state.stack().write().unwrap().push(json);
    }
    fn session(&mut self, attributes: AttributeMap) -> io::Result<()> {
        if let Some(Some(session_name)) = attributes.get(b"name".as_ref()) {
            let session_name = session_name.to_str();
            if session_name != "" {
                let commit_on_close =
                    if let Some(Some(col)) = attributes.get(b"commit_on_close".as_ref()) {
                        col.to_str() == "true"
                    } else {
                        false
                    };

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
                    self.database.read().unwrap().session(&session_name, expire)
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
                    self.sessions.push(SessionState {
                        session,
                        commit_on_close,
                        clear_on_close,
                    });
                }
            }
        }
        Ok(())
    }
    fn sessions(&mut self, attributes: &AttributeMap) {
        let mut json = HashMap::new();

        if let Some(Some(var)) = attributes.get(b"var".as_ref()) {
            let var = var.to_str();
            if var != "" {
                if let Ok(sessions) = self.database.read().unwrap().sessions() {
                    json.insert(
                        var.to_string().as_bytes().to_vec(),
                        Arc::new(RwLock::new(WildDocValue::new(serde_json::json!(sessions)))),
                    );
                }
            }
        }
        self.state.stack().write().unwrap().push(json);
    }
    fn session_sequence(&mut self, attributes: AttributeMap) -> io::Result<()> {
        let mut str_max = if let Some(Some(s)) = attributes.get(b"max".as_ref()) {
            s.to_str()
        } else {
            "".into()
        };
        if str_max == "" {
            str_max = "session_sequence_max".into();
        }

        let mut str_current = if let Some(Some(s)) = attributes.get(b"current".as_ref()) {
            s.to_str()
        } else {
            "".into()
        };
        if str_current == "" {
            str_current = "session_sequence_current".into();
        }

        let mut json = HashMap::new();
        if let Some(session_state) = self.sessions.last() {
            if let Some(cursor) = session_state.session.sequence_cursor() {
                json.insert(
                    str_max.as_bytes().to_vec(),
                    Arc::new(RwLock::new(WildDocValue::new(serde_json::json!(
                        cursor.max
                    )))),
                );

                json.insert(
                    str_current.as_bytes().to_vec(),
                    Arc::new(RwLock::new(WildDocValue::new(serde_json::json!(
                        cursor.current
                    )))),
                );
            }
        }
        self.state.stack().write().unwrap().push(json);

        Ok(())
    }
    fn session_gc(&mut self, attributes: AttributeMap) -> io::Result<()> {
        let mut expire = 60 * 60 * 24;
        if let Some(Some(str_expire)) = attributes.get(b"expire".as_ref()) {
            if let Ok(parsed) = str_expire.to_str().parse::<i64>() {
                expire = parsed;
            }
        }
        self.database.write().unwrap().session_gc(expire)
    }
    fn delete_collection(&mut self, attributes: AttributeMap) -> Result<()> {
        if let Some(Some(str_collection)) = attributes.get(b"collection".as_ref()) {
            self.database
                .clone()
                .write()
                .unwrap()
                .delete_collection(str_collection.to_str().as_ref())?;
        }
        Ok(())
    }

    fn custom_tag(&mut self, attributes: AttributeMap) -> (String, Vec<u8>) {
        let mut html_attr = vec![];
        let mut name = "".to_string();
        for (key, value) in attributes {
            if let Some(value) = value {
                if key.starts_with(b"wd-tag:name") {
                    name = value.to_str().to_string();
                } else if key.starts_with(b"wd-attr:replace") {
                    let attr = xml_util::quot_unescape(value.to_str().as_bytes());
                    if attr.len() > 0 {
                        html_attr.push(b' ');
                        html_attr.append(&mut attr.as_bytes().to_vec());
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
