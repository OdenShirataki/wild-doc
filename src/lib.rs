use std::sync::{Arc,RwLock};

use quick_xml::{
    Reader
    ,events::Event
};
use semilattice_database::Database;

mod script;
use script::Script;

mod xml_util;
use xml_util::XmlAttr;

mod include;
pub use include::{IncludeAdaptor,IncludeLocal};

pub struct SemilatticeScript<T:IncludeAdaptor>{
    database:Arc<RwLock<Database>>
    ,include_adaptor:T
}
impl<T:IncludeAdaptor> SemilatticeScript<T>{
    pub fn new(
        dir:&str
        ,include_adaptor:T
    )->Result<Self,std::io::Error>{
        Ok(Self{
            database:Arc::new(RwLock::new(Database::new(dir)?))
            ,include_adaptor
        })
    }
    pub fn exec(&mut self,qml:&str)->String{
        //println!("{}",qml);
        let mut reader=Reader::from_str(qml.trim());
        reader.expand_empty_elements(true);
        loop{
            match reader.read_event(){
                Ok(Event::Start(e))=>{
                    if e.name().as_ref()==b"wd"{
                        let mut script=Script::new(
                            self.database.clone()
                        );
                        return script.parse_xml(&mut reader,&self.include_adaptor);
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
    let wdkey="wd:".to_owned()+key;
    if let Some(value)=attr.get(&wdkey){
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