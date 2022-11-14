use std::sync::Arc;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::{Once, RwLock};

use quick_xml::events::{Event, BytesStart};
use v8;

use quick_xml::Reader;
use semilattice_database::{Database, Session};

mod process;

use crate::{xml_util, IncludeAdaptor, attr_parse_or_static};
mod update;
mod search;
mod method;
use method::*;

pub struct Script{
    database:Arc<RwLock<Database>>
    ,sessions:Vec<Session>
}
impl Script{
    pub fn new(
        database:Arc<RwLock<Database>>
    )->Self{
        static START: Once=Once::new();
        START.call_once(||{
            v8::V8::set_flags_from_string("--expose_gc");
            v8::V8::initialize_platform(
                v8::new_default_platform(0,false).make_shared()
            );
            v8::V8::initialize();
        });
        let session=database.clone().read().unwrap().blank_session().unwrap();
        Self{
            database
            ,sessions:vec![session]
        }
    }
    pub fn parse_xml<T:IncludeAdaptor>(&mut self,reader: &mut Reader<&[u8]>,include_adaptor:&mut T)->Result<String,std::io::Error>{
        let params = v8::Isolate::create_params();
        let mut isolate = v8::Isolate::new(params);

        isolate.set_slot(self.database.clone());

        let scope=&mut v8::HandleScope::new(&mut isolate);
        let context=v8::Context::new(scope);
        let scope=&mut v8::ContextScope::new(scope,context);

        let global=context.global(scope);

        let mut ret="".to_string();
        if let (
            Some(v8str_wd)
            ,Some(v8str_stack)
            ,Some(v8str_v)
            ,Some(func_v)
        )=(
            v8::String::new(scope,"wd")
            ,v8::String::new(scope,"stack")
            ,v8::String::new(scope,"v")
            ,v8::Function::new(scope,v)
        ){
            let wd=v8::Object::new(scope);
            let stack=v8::Array::new(scope,0);
            wd.set(scope,v8str_stack.into(),stack.into());
            wd.set(scope,v8str_v.into(),func_v.into());
            
            global.set(
                scope
                ,v8str_wd.into()
                ,wd.into()
            );
            
            ret=self.parse(
                scope
                ,reader
                ,"wd"
                ,include_adaptor
            )?;
        }
        Ok(ret)
    }

    pub fn parse<T:IncludeAdaptor>(&mut self,scope: &mut v8::HandleScope,reader: &mut Reader<&[u8]>,break_tag:&str,include_adaptor:&mut T)->Result<String,std::io::Error>{
        let mut search_map=HashMap::new();
        let mut r=String::new();
        loop{
            if let Ok(next)=reader.read_event(){
                match next{
                    Event::Start(ref e)=>{
                        let name=e.name();
                        let name=name.as_ref();
                        match name{
                            b"wd:session"=>{
                                let attr=xml_util::attr2hash_map(&e);
                                let session_name=crate::attr_parse_or_static(scope,&attr,"name");
                                if let Ok(mut session)=Session::new(&self.database.clone().read().unwrap(),&session_name){
                                    if session_name!=""{
                                        if let Ok(Some(value))=e.try_get_attribute(b"initialize"){
                                            if value.value.to_vec()==b"true"{
                                                self.database.clone().read().unwrap().session_restart(&mut session)?;
                                            }
                                        }
                                    }
                                    self.sessions.push(session);
                                }else{
                                    xml_util::outer(&next,reader);
                                }
                            }
                            ,b"wd:update"=>{
                                let attr=xml_util::attr2hash_map(&e);
                                let with_commit=crate::attr_parse_or_static(scope,&attr,"commit")=="1";
                                
                                let inner_xml=self.parse(scope,reader,"wd:update",include_adaptor)?;
                                let mut inner_reader=Reader::from_str(&inner_xml);
                                let updates=update::make_update_struct(self,&mut inner_reader,scope);
                                if let Some(session)=self.sessions.last_mut(){
                                    if !session.is_blank(){
                                        self.database.clone().read().unwrap().update(session,updates)?;
                                        if with_commit{
                                            self.database.clone().write().unwrap().commit(session)?;
                                        }
                                    }
                                }
                            }
                            ,b"wd:search"=>{
                                let attr=xml_util::attr2hash_map(&e);
                                let name=crate::attr_parse_or_static(scope,&attr,"name");
                                let collection_name=crate::attr_parse_or_static(scope,&attr,"collection");
                                
                                if name!="" && collection_name!=""{
                                    if let Some(collection_id)=self.database.clone().read().unwrap().collection_id(&collection_name){
                                        let condition=search::make_conditions(self,&attr,reader,scope);
                                        search_map.insert(name.to_owned(),(collection_id,condition));
                                    }
                                }
                            }
                            ,b"wd:result"=>{
                                let attr=xml_util::attr2hash_map(&e);
                                let search=crate::attr_parse_or_static(scope,&attr,"search");
                                let var=crate::attr_parse_or_static(scope,&attr,"var");
                                if search!="" && var!=""{
                                    if let Some((collection_id,conditions))=search_map.get(&search){
                                        let collection_id=*collection_id;
                                        if let Some(collection)=self.database.clone().read().unwrap().collection(collection_id){
                                            let mut search=self.database.clone().read().unwrap().search(collection);
                                            for c in conditions{
                                                search=search.search(c.clone());
                                            }
                                            let rowset=self.database.clone().read().unwrap().result(&search);
                                            let context=scope.get_current_context();
                                            let global=context.global(scope);
                                            if let (
                                                Some(str_collection_id)
                                                ,Some(v8str_session_key)
                                                ,Some(v8str_func_field)
                                                ,Some(v8str_row)
                                                ,Some(v8str_wd)
                                                ,Some(v8str_stack)
                                                ,Some(var)
                                                ,Some(v8func_field)
                                            )=(
                                                v8::String::new(scope,"collection_id")
                                                ,v8::String::new(scope,"session_key")
                                                ,v8::String::new(scope,"field")
                                                ,v8::String::new(scope,"row")
                                                ,v8::String::new(scope,"wd")
                                                ,v8::String::new(scope,"stack")
                                                ,v8::String::new(scope,&var)
                                                ,v8::Function::new(scope,field)
                                            ){
                                                if let Some(wd)=global.get(scope,v8str_wd.into()){
                                                    if let Ok(wd)=v8::Local::<v8::Object>::try_from(wd){
                                                        if let Some(stack)=wd.get(scope,v8str_stack.into()){
                                                            if let Ok(stack)=v8::Local::<v8::Array>::try_from(stack){
                                                                let obj=v8::Object::new(scope);
                                                                let return_obj=v8::Array::new(scope,0); 
                                                                let mut i=0;
                                                                for d in rowset{
                                                                    let obj=v8::Object::new(scope);
                                                                    let row=v8::Integer::new(scope, d as i32);
                                                                    let collection_id=v8::Integer::new(scope,collection_id as i32);
                                                                    obj.set(scope,v8str_row.into(),row.into());
                                                                    obj.set(scope,v8str_func_field.into(),v8func_field.into());
                                                                    obj.set(scope,str_collection_id.into(),collection_id.into());
                                                                    if let Some(session_key)=obj.get(scope,v8str_session_key.into()){ 
                                                                        obj.set(
                                                                            scope
                                                                            ,v8str_session_key.into()
                                                                            ,session_key.into()
                                                                        );
                                                                    }
                                                                    return_obj.set_index(scope,i,obj.into());
                                                                    i+=1;
                                                                }
                                                                obj.set(scope,var.into(),return_obj.into());
                                                                stack.set_index(scope,stack.length(),obj.into());
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            ,b"wd:stack"=>{
                                if let Ok(Some(var))=e.try_get_attribute(b"var"){
                                    std::str::from_utf8(&var.value).ok()
                                        .and_then(|code|v8::String::new(scope,&("wd.stack.push({".to_owned()+&code+"});"))
                                        .and_then(|code|v8::Script::compile(scope, code, None))
                                        .and_then(|v|v.run(scope)))
                                    ;
                                }
                            }
                            ,b"wd:script"=>{
                                v8::String::new(scope,&match reader.read_event(){
                                    Ok(Event::Text(c))=>std::str::from_utf8(&c.into_inner()).unwrap_or("").trim().to_string()
                                    ,Ok(Event::CData(c))=>std::str::from_utf8(&c.into_inner()).unwrap_or("").trim().to_string()
                                    ,_=> "".to_string()
                                })
                                    .and_then(|code|v8::Script::compile(scope, code, None))
                                    .and_then(|v|v.run(scope))
                                ;
                            }
                            ,b"wd:case"=>{
                                r+=&process::case(self,&e,&xml_util::outer(&next,reader),scope,include_adaptor)?;
                            }
                            ,b"wd:for"=>{
                                let outer=xml_util::outer(&next,reader);
                                r+=&process::r#for(self,&e,&outer,scope,include_adaptor)?;
                            }
                            ,_=>{
                                if !name.starts_with(b"wd:"){
                                    let html_attr=Self::html_attr(e,scope);
                                    r+=&("<".to_owned()+std::str::from_utf8(name).unwrap_or("")+&html_attr+">");
                                }
                            }
                        }
                    }
                    ,Event::Empty(ref e)=>{
                        let name=e.name();
                        let name=name.as_ref();
                        match name{
                            b"wd:print"=>{
                                let attr=xml_util::attr2hash_map(e);
                                r+=&crate::attr_parse_or_static(scope,&attr,"value");
                            }
                            ,b"wd:include"=>{
                                let attr=xml_util::attr2hash_map(e);
                                let src=attr_parse_or_static(scope,&attr,"src");
                                let xml=include_adaptor.include(&src);
                                if xml.len()>0{
                                    let str_xml="<root>".to_owned()+&xml+"</root>";
                                    let mut event_reader_inner=Reader::from_str(&str_xml);
                                    loop{
                                        match event_reader_inner.read_event(){
                                            Ok(Event::Start(e))=>{
                                                if e.name().as_ref()==b"root"{
                                                    r+=&self.parse(scope,&mut event_reader_inner,"root",include_adaptor)?;
                                                    break;
                                                }
                                            }
                                            ,_=>{}
                                        }
                                    }
                                }
                            }
                            ,_=>{
                                if !name.starts_with(b"wd:"){
                                    let html_attr=Self::html_attr(e,scope);
                                    r+=&("<".to_owned()+std::str::from_utf8(name).unwrap_or("")+&html_attr+" />");
                                }
                            }
                        }
                    }
                    ,Event::End(e)=>{
                        let name=e.name();
                        let name=name.as_ref();
                        if name==b"wd" || name==break_tag.as_bytes(){
                            break;
                        }else{
                            if name.starts_with(b"wd:"){
                                if name==b"wd:stack"{
                                    v8::String::new(scope,"wd.stack.pop();")
                                        .and_then(|code|v8::Script::compile(scope, code, None))
                                        .and_then(|v|v.run(scope))
                                    ;
                                }else if name==b"wd:session"{
                                    self.sessions.pop();
                                }
                            }else{
                                r+=&("</".to_owned()+std::str::from_utf8(name).unwrap_or("")+">");
                            }
                        }
                    }
                    ,Event::CData(c)=>{
                        r+=std::str::from_utf8(&c.into_inner()).unwrap_or("");
                    }
                    ,Event::Text(c)=>{
                        r+=&c.unescape().expect("Error!");
                    }
                    ,Event::Eof=>{
                        break;
                    }
                    ,_ => {}
                }
            }
        }
        Ok(r)
    }

    fn html_attr(e:&BytesStart,scope:&mut v8::HandleScope)->String{
        let mut html_attr="".to_string();
        for attr in e.attributes(){
            if let Ok(attr)=attr{
                if let Ok(attr_key)=std::str::from_utf8(attr.key.as_ref()){
                    let is_wd=attr_key.starts_with("wd:");
                    let attr_key=if is_wd{
                        attr_key.split_at(3).1
                    }else{
                        attr_key
                    };
                    html_attr.push(' ');
                    html_attr.push_str(attr_key);
                    html_attr.push_str("=\"");
                    
                    if let Ok(value)=std::str::from_utf8(&attr.value){
                        if is_wd{
                            html_attr.push_str(&crate::eval_result(scope, value));
                        }else{
                            html_attr.push_str(
                                &value.replace("&","&amp;").replace("<","&lt;").replace(">","&gt;")
                            );
                        }
                    }
                    html_attr.push('"');
                }
            }
        }
        html_attr
    }
}

