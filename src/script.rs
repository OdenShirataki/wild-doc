use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::rc::Rc;
use std::sync::Once;

use quick_xml::events::Event;
use v8;

use quick_xml::Reader;
use semilattice_database::{Database, Session};

mod process;

use crate::xml_util;
mod update;
mod search;
mod method;
use method::*;

pub struct Script{
    database:Rc<RefCell<Database>>
    ,sessions:Vec<Session>
}
impl Script{
    pub fn new(database:Rc<RefCell<Database>>)->Script{
        let session=database.clone().borrow().blank_session().unwrap();
        Script{
            database
            ,sessions:vec![session]
        }
    }
    pub fn parse_xml(&mut self,reader: &mut Reader<&[u8]>)->String{
        static START: Once = Once::new();
        START.call_once(||{
            v8::V8::set_flags_from_string("--expose_gc");
            v8::V8::initialize_platform(
                v8::new_default_platform(0,false).make_shared()
            );
            v8::V8::initialize();
        });

        let params = v8::Isolate::create_params();
        let mut isolate = v8::Isolate::new(params);

        isolate.set_slot(self.database.clone());

        let scope=&mut v8::HandleScope::new(&mut isolate);
        let context=v8::Context::new(scope);
        let scope=&mut v8::ContextScope::new(scope,context);

        let global=context.global(scope);

        let mut ret="".to_string();
        if let (
            Some(v8str_ss)
            ,Some(v8str_stack)
            ,Some(v8str_v)
            ,Some(func_v)
        )=(
            v8::String::new(scope,"ss")
            ,v8::String::new(scope,"stack")
            ,v8::String::new(scope,"v")
            ,v8::Function::new(scope,v)
        ){
            let ss=v8::Object::new(scope);
            let stack=v8::Array::new(scope,0);
            ss.set(scope,v8str_stack.into(),stack.into());
            ss.set(scope,v8str_v.into(),func_v.into());
            
            global.set(
                scope
                ,v8str_ss.into()
                ,ss.into()
            );
            
            ret=self.parse(
                scope
                ,reader
                ,"ss"
            );
        }
        ret
    }

    pub fn parse(&mut self,scope: &mut v8::HandleScope,reader: &mut Reader<&[u8]>,break_tag:&str)->String{
        let mut search_map=HashMap::new();
        let mut r=String::new();
        loop{
            if let Ok(next)=reader.read_event(){
                match next{
                    Event::Start(ref e)=>{
                        let name=e.name();
                        match name.as_ref(){
                            b"ss:session"=>{
                                let attr=xml_util::attr2hash_map(&e);
                                let session_name=crate::attr_parse_or_static(scope,&attr,"name");
                                if let Ok(mut session)=Session::new(&self.database.clone().borrow(),&session_name){
                                    if session_name!=""{
                                        if let Ok(Some(value))=e.try_get_attribute(b"initialize"){
                                            if value.value.to_vec()==b"true"{
                                                self.database.clone().borrow().session_restart(&mut session);
                                            }
                                        }
                                    }
                                    self.sessions.push(session);
                                }else{
                                    xml_util::outer(&next,reader);
                                }
                            }
                            ,b"ss:update"=>{
                                let attr=xml_util::attr2hash_map(&e);
                                let with_commit=crate::attr_parse_or_static(scope,&attr,"commit")=="1";
                                
                                let inner_xml=self.parse(scope,reader,"ss:update");
                                let mut inner_reader=Reader::from_str(&inner_xml);
                                inner_reader.expand_empty_elements(true);
                                let updates=update::make_update_struct(self,&mut inner_reader,scope);

                                if let Some(session)=self.sessions.last_mut(){
                                    if !session.is_blank(){
                                        self.database.clone().borrow_mut().update(session,updates);
                                        if with_commit{
                                            self.database.clone().borrow_mut().commit(session);
                                        }
                                    }
                                }
                            }
                            ,b"ss:search"=>{
                                let attr=xml_util::attr2hash_map(&e);
                                let name=crate::attr_parse_or_static(scope,&attr,"name");
                                let collection_name=crate::attr_parse_or_static(scope,&attr,"collection");
                                
                                if name!="" && collection_name!=""{
                                    if let Some(collection_id)=self.database.clone().borrow().collection_id(&collection_name){
                                        let condition=search::make_conditions(self,&attr,reader,scope);
                                        search_map.insert(name.to_owned(),(collection_id,condition));
                                    }
                                }
                            }
                            ,b"ss:result"=>{
                                let attr=xml_util::attr2hash_map(&e);
                                let search=crate::attr_parse_or_static(scope,&attr,"search");
                                let var=crate::attr_parse_or_static(scope,&attr,"var");
                                if search!="" && var!=""{
                                    if let Some((collection_id,conditions))=search_map.get(&search){
                                        let collection_id=*collection_id;
                                        if let Some(collection)=self.database.clone().borrow().collection(collection_id){
                                            let mut search=self.database.clone().borrow().search(collection);
                                            for c in conditions{
                                                search=search.search(c.clone());
                                            }
                                            let rowset=self.database.clone().borrow().result(&search);
                                            let context=scope.get_current_context();
                                            let global=context.global(scope);
                                            if let (
                                                Some(str_collection_id)
                                                ,Some(v8str_session_key)
                                                ,Some(v8str_func_field)
                                                ,Some(v8str_row)
                                                ,Some(v8str_ss)
                                                ,Some(v8str_stack)
                                                ,Some(var)
                                                ,Some(v8func_field)
                                            )=(
                                                v8::String::new(scope,"collection_id")
                                                ,v8::String::new(scope,"session_key")
                                                ,v8::String::new(scope,"field")
                                                ,v8::String::new(scope,"row")
                                                ,v8::String::new(scope,"ss")
                                                ,v8::String::new(scope,"stack")
                                                ,v8::String::new(scope,&var)
                                                ,v8::Function::new(scope,field)
                                            ){
                                                if let Some(ss)=global.get(scope,v8str_ss.into()){
                                                    if let Ok(ss)=v8::Local::<v8::Object>::try_from(ss){
                                                        if let Some(stack)=ss.get(scope,v8str_stack.into()){
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
                            ,b"ss:stack"=>{
                                if let Ok(Some(var))=e.try_get_attribute(b"var"){
                                    std::str::from_utf8(&var.value).ok()
                                        .and_then(|code|v8::String::new(scope,&("ss.stack.push({".to_owned()+&code+"});"))
                                        .and_then(|code|v8::Script::compile(scope, code, None))
                                        .and_then(|v|v.run(scope)))
                                    ;
                                }
                            }
                            ,b"ss:script"=>{
                                v8::String::new(scope,&match reader.read_event(){
                                    Ok(Event::Text(c))=>std::str::from_utf8(&c.into_inner()).unwrap_or("").trim().to_string()
                                    ,Ok(Event::CData(c))=>std::str::from_utf8(&c.into_inner()).unwrap_or("").trim().to_string()
                                    ,_=> "".to_string()
                                })
                                    .and_then(|code|v8::Script::compile(scope, code, None))
                                    .and_then(|v|v.run(scope))
                                ;
                            }
                            ,b"ss:print"=>{
                                let attr=xml_util::attr2hash_map(&e);
                                r+=&crate::attr_parse_or_static(scope,&attr,"value");
                            }
                            ,b"ss:case"=>{
                                r+=&process::case(self,&e,&xml_util::outer(&next,reader),scope);
                            }
                            ,b"ss:for"=>{
                                let outer=xml_util::outer(&next,reader);
                                r+=&process::r#for(self,&e,&outer,scope);
                            }
                            /*
                            ,b"ss:include"=>{
                                r+=&process::include(session,&e,scope);
                            }
                            */
                            ,_=>{
                                r+=&process::html(e.name().as_ref(),&e,scope);
                            }
                        }
                    }
                    ,Event::Eof=>{
                        break;
                    }
                    ,Event::End(e)=>{
                        let name=e.name();
                        let name=name.as_ref();
                        if name==b"ss" || name==break_tag.as_bytes(){
                            break;
                        }else{
                            if name.starts_with(b"ss:"){
                                if name==b"ss:stack"{
                                    v8::String::new(scope,"ss.stack.pop();")
                                        .and_then(|code|v8::Script::compile(scope, code, None))
                                        .and_then(|v|v.run(scope))
                                    ;
                                }else if name==b"ss:session"{
                                    self.sessions.pop();
                                }
                            }else{
                                r.push_str("</");
                                r.push_str(std::str::from_utf8(name).unwrap_or(""));
                                r.push('>');
                            }
                        }
                    }
                    ,Event::CData(c)=>{
                        r.push_str(std::str::from_utf8(&c.into_inner()).expect("Error!"));
                    }
                    ,Event::Text(c)=>{
                        r.push_str(&c.unescape().expect("Error!"));
                    }
                    ,_ => {}
                }
            }
        }
        r
    }
}