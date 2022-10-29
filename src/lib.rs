use std::{rc::Rc, cell::RefCell};

use quick_xml::{
    Reader
    ,events::Event
};
use semilattice_database::Database;

mod script;
use script::Script;
use xml_util::XmlAttr;

mod xml_util;

pub struct SemilatticeScript{
    database:Rc<RefCell<Database>>
}
impl SemilatticeScript{
    pub fn new(dir:&str)->Result<SemilatticeScript,std::io::Error>{
        Ok(SemilatticeScript{
            database:Rc::new(RefCell::new(Database::new(dir)?))
        })
    }

    pub fn exec(&mut self,qml:&str)->String{
        let mut reader=Reader::from_str(qml.trim());
        reader.expand_empty_elements(true);
        loop{
            match reader.read_event(){
                Ok(Event::Start(e))=>{
                    if e.name().as_ref()==b"ss"{
                        let mut script=Script::new(
                            self.database.clone()
                        );
                        return script.parse_xml(&mut reader);
                    }
                }
                ,_=>{}
            }
        }
    }
}

fn eval<'s>(scope: &mut v8::HandleScope<'s>,code: &str) -> Option<v8::Local<'s, v8::Value>> {
    let scope = &mut v8::EscapableHandleScope::new(scope);
    let source = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, source, None).unwrap();
    let r = script.run(scope);
    r.map(|v| scope.escape(v))
}

fn eval_result(scope:&mut v8::HandleScope,value:&str)->String{
    if let Some(v8_value)=v8::String::new(scope,value)
        .and_then(|code|v8::Script::compile(scope, code, None))
        .and_then(|v|v.run(scope))
        .and_then(|v|v.to_string(scope))
    {
        v8_value.to_rust_string_lossy(scope)
    }else{
        value.to_string()
    }
}

fn attr_parse_or_static(scope:&mut v8::HandleScope,attr:&XmlAttr,key:&str)->String{
    let sskey="ss:".to_owned()+key;
    if let Some(value)=attr.get(&sskey){
        if let Ok(value)=std::str::from_utf8(value){
            crate::eval_result(scope,value)
        }else{
            "".to_owned()
        }
    }else if let Some(value)=attr.get(key){
        if let Ok(value)=std::str::from_utf8(value){
            value
        }else{
            ""
        }.to_owned()
    }else{
        "".to_owned()
    }
}