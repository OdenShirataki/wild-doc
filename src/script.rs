use std::collections::HashMap;
use std::rc::Rc;
use std::convert::TryFrom;
use std::sync::{Arc,RwLock};

use deno_runtime::{
    deno_core::{
        self
        ,v8
        ,ModuleSpecifier
        ,error::AnyError
    }
    ,BootstrapOptions
    ,deno_broadcast_channel::InMemoryBroadcastChannel
    ,deno_web::BlobStore
    ,permissions::Permissions
    ,worker::{WorkerOptions, MainWorker}
    ,ops
};
use quick_xml::events::{Event, BytesStart};

use quick_xml::Reader;
use semilattice_database::{Database, Session};

mod process;

use crate::{xml_util, IncludeAdaptor};
mod update;
mod search;
mod method;
use method::*;

mod module_loader;
use module_loader::WdModuleLoader;

fn get_error_class_name(e: &AnyError) -> &'static str {
    deno_runtime::errors::get_error_class_name(e).unwrap_or("Error")
}

pub struct Script{
    database:Arc<RwLock<Database>>
    ,sessions:Vec<Session>
    ,main_module:ModuleSpecifier
    ,module_loader:Rc<WdModuleLoader>
    ,bootstrap:BootstrapOptions
    ,permissions:Permissions
    ,create_web_worker_cb: Arc<ops::worker_host::CreateWebWorkerCb>
    ,web_worker_event_cb: Arc<ops::worker_host::WorkerEventCb>
}
impl Script{
    pub fn new(
        database:Arc<RwLock<Database>>
    )->Self{
        let session=database.clone().read().unwrap().blank_session().unwrap();
        Self{
            database
            ,sessions:vec![session]
            ,main_module:deno_core::resolve_path("mainworker").unwrap()
            ,module_loader:WdModuleLoader::new()
            ,bootstrap: BootstrapOptions {
                args: vec![],
                cpu_count: 1,
                debug_flag: false,
                enable_testing_features: false,
                locale: v8::icu::get_language_tag(),
                location: None,
                no_color: false,
                is_tty: false,
                runtime_version: "x".to_string(),
                ts_version: "x".to_string(),
                unstable: false,
                user_agent: "hello_runtime".to_string(),
                inspect: false,
            }
            ,permissions:Permissions::allow_all()
            ,create_web_worker_cb: Arc::new(|_| {
                todo!("Web workers are not supported in the example");
            })
            ,web_worker_event_cb: Arc::new(|_| {
                todo!("Web workers are not supported in the example");
            })
        }
    }
    pub fn parse_xml<T:IncludeAdaptor>(&mut self,input_json:&str,reader: &mut Reader<&[u8]>,include_adaptor:&mut T)->Result<super::WildDocResult,std::io::Error>{
        let options = WorkerOptions {
            bootstrap: self.bootstrap.clone(),
            extensions: vec![],
            startup_snapshot: None,
            unsafely_ignore_certificate_errors: None,
            root_cert_store: None,
            seed: None,
            source_map_getter: None,
            format_js_error_fn: None,
            web_worker_preload_module_cb: self.web_worker_event_cb.clone(),
            web_worker_pre_execute_module_cb: self.web_worker_event_cb.clone(),
            create_web_worker_cb:self.create_web_worker_cb.clone(),
            maybe_inspector_server: None,
            should_break_on_first_statement: false,
            module_loader:self.module_loader.clone(),
            npm_resolver: None,
            get_error_class_fn: Some(&get_error_class_name),
            cache_storage_dir: None,
            origin_storage_dir: None,
            blob_store: BlobStore::default(),
            broadcast_channel: InMemoryBroadcastChannel::default(),
            shared_array_buffer_store: None,
            compiled_wasm_module_store: None,
            stdio: Default::default(),
        };

        let mut worker = MainWorker::bootstrap_from_options(
            self.main_module.clone()
            ,self.permissions.clone()
            ,options
        );
        worker.js_runtime.v8_isolate().set_slot(self.database.clone());
        let _=worker.execute_script("init",&(
r#"wd={
    general:{}
    ,stack:[]
    ,result_options:{}
    ,input:"#.to_owned()+(
        if input_json.len()>0{
            input_json
        }else{
            "{}"
        }
    )+r#"
};
wd.v=key=>{
    for(let i=wd.stack.length-1;i>=0;i--){
        if(wd.stack[i][key]!==void 0){
            return wd.stack[i][key];
        }
    }
};"#
        ));
        let result_body=self.parse(&mut worker,reader,"wd",include_adaptor)?;
        let result_options={
            let mut result_options=String::new();
            let scope=&mut worker.js_runtime.handle_scope();
            let context=scope.get_current_context();
            let scope=&mut v8::ContextScope::new(scope,context);
            if let Some(v)=v8::String::new(scope,"wd.result_options")
                .and_then(|code|v8::Script::compile(scope, code, None))
                .and_then(|v|v.run(scope))
            {
                if let Some(json)=v8::json::stringify(scope,v){
                    result_options=json.to_rust_string_lossy(scope);
                }
            }
            result_options
        };
        Ok(super::WildDocResult{
            body:result_body
            ,options_json:result_options
        })
    }
    pub fn parse<T:IncludeAdaptor>(&mut self,worker: &mut MainWorker,reader: &mut Reader<&[u8]>,break_tag:&str,include_adaptor:&mut T)->Result<Vec<u8>,std::io::Error>{
        let mut search_map=HashMap::new();
        let mut r=Vec::new();
        loop{
            if let Ok(next)=reader.read_event(){
                match next{
                    Event::Start(ref e)=>{
                        let name=e.name();
                        let name=name.as_ref();
                        match name{
                            b"wd:session"=>{
                                let attr=xml_util::attr2hash_map(&e);
                                let session_name=crate::attr_parse_or_static(worker,&attr,"name");
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
                                let with_commit=crate::attr_parse_or_static(worker,&attr,"commit")=="1";
                                
                                let inner_xml=self.parse(worker,reader,"wd:update",include_adaptor)?;
                                let mut inner_reader=Reader::from_str(std::str::from_utf8(&inner_xml).unwrap());
                                let updates=update::make_update_struct(self,&mut inner_reader,worker);
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
                                let name=crate::attr_parse_or_static(worker,&attr,"name");
                                let collection_name=crate::attr_parse_or_static(worker,&attr,"collection");
                                
                                if name!="" && collection_name!=""{
                                    if let Some(collection_id)=self.database.clone().read().unwrap().collection_id(&collection_name){
                                        let condition=search::make_conditions(self,&attr,reader,worker);
                                        search_map.insert(name.to_owned(),(collection_id,condition));
                                    }
                                }
                            }
                            ,b"wd:result"=>{
                                let attr=xml_util::attr2hash_map(&e);
                                let search=crate::attr_parse_or_static(worker,&attr,"search");
                                let var=crate::attr_parse_or_static(worker,&attr,"var");

                                let scope=&mut worker.js_runtime.handle_scope();
                                let context=scope.get_current_context();
                                let scope=&mut v8::ContextScope::new(scope,context);
                                if search!="" && var!=""{
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
                                        let context=scope.get_current_context();
                                        let global=context.global(scope);
                                        if let Some(wd)=global.get(scope,v8str_wd.into()){
                                            if let Ok(wd)=v8::Local::<v8::Object>::try_from(wd){
                                                if let Some(stack)=wd.get(scope,v8str_stack.into()){
                                                    if let Ok(stack)=v8::Local::<v8::Array>::try_from(stack){
                                                        let obj=v8::Object::new(scope);
                                                        let return_obj=v8::Array::new(scope,0); 
                                                        if let Some((collection_id,conditions))=search_map.get(&search){
                                                            let collection_id=*collection_id;
                                                            if let Some(collection)=self.database.clone().read().unwrap().collection(collection_id){
                                                                let mut search=self.database.clone().read().unwrap().search(collection);
                                                                for c in conditions{
                                                                    search=search.search(c.clone());
                                                                }
                                                                let rowset=self.database.clone().read().unwrap().result(&search);
                                                        
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
                                                            }
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
                            ,b"wd:stack"=>{
                                if let Ok(Some(var))=e.try_get_attribute(b"var"){
                                    let scope=&mut worker.js_runtime.handle_scope();
                                    let context=scope.get_current_context();
                                    let scope=&mut v8::ContextScope::new(scope,context);
                                    std::str::from_utf8(&var.value).ok()
                                        .and_then(|code|v8::String::new(scope,&("wd.stack.push({".to_owned()+&code+"});"))
                                        .and_then(|code|v8::Script::compile(scope, code, None))
                                        .and_then(|v|v.run(scope)))
                                    ;
                                }
                            }
                            ,b"wd:script"=>{
                                //TODO: use reader.read_to_end
                                let src=match reader.read_event(){
                                    Ok(Event::Text(c))=>std::str::from_utf8(&c.into_inner()).unwrap_or("").trim().to_string()
                                    ,Ok(Event::CData(c))=>std::str::from_utf8(&c.into_inner()).unwrap_or("").trim().to_string()
                                    ,_=> "".to_string()
                                };
                                deno_core::futures::executor::block_on(async{
                                    let n=ModuleSpecifier::parse("wd://script").unwrap();
                                    if let Ok(mod_id) = worker.js_runtime.load_side_module(&n, Some(src)).await{
                                        let result = worker.js_runtime.mod_evaluate(mod_id);
                                        let _=worker.run_event_loop(false).await;
                                        let _=result.await;
                                    }
                                });
                            }
                            ,b"wd:case"=>{
                                r.append(&mut process::case(self,&e,&xml_util::outer(&next,reader),worker,include_adaptor)?);
                            }
                            ,b"wd:for"=>{
                                let outer=xml_util::outer(&next,reader);
                                r.append(&mut process::r#for(self,&e,&outer,worker,include_adaptor)?);
                            }
                            ,_=>{
                                if !name.starts_with(b"wd:"){
                                    let scope=&mut worker.js_runtime.handle_scope();
                                    let context=scope.get_current_context();
                                    let scope=&mut v8::ContextScope::new(scope,context);
                                    let html_attr=Self::html_attr(e,scope);
                                    r.push(b'<');
                                    r.append(&mut name.to_vec());
                                    r.append(&mut html_attr.as_bytes().to_vec());
                                    r.push(b'>');
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
                                r.append(&mut crate::attr_parse_or_static(worker,&attr,"value").as_bytes().to_vec());
                            }
                            ,b"wd:include"=>{
                                let attr=xml_util::attr2hash_map(e);
                                let src=crate::attr_parse_or_static(worker,&attr,"src");
                                let xml=include_adaptor.include(&src);
                                if xml.len()>0{
                                    let str_xml="<root>".to_owned()+&xml+"</root>";
                                    let mut event_reader_inner=Reader::from_str(&str_xml);
                                    loop{
                                        match event_reader_inner.read_event(){
                                            Ok(Event::Start(e))=>{
                                                if e.name().as_ref()==b"root"{
                                                    r.append(&mut self.parse(worker,&mut event_reader_inner,"root",include_adaptor)?);
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
                                    let scope=&mut worker.js_runtime.handle_scope();
                                    let context=scope.get_current_context();
                                    let scope=&mut v8::ContextScope::new(scope,context);
                                    let html_attr=Self::html_attr(e,scope);
                                    r.push(b'<');
                                    r.append(&mut name.to_vec());
                                    r.append(&mut html_attr.as_bytes().to_vec());
                                    r.append(&mut b" />".to_vec());
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
                                    let scope=&mut worker.js_runtime.handle_scope();
                                    let context=scope.get_current_context();
                                    let scope=&mut v8::ContextScope::new(scope,context);
                                    v8::String::new(scope,"wd.stack.pop();")
                                        .and_then(|code|v8::Script::compile(scope, code, None))
                                        .and_then(|v|v.run(scope))
                                    ;
                                }else if name==b"wd:session"{
                                    self.sessions.pop();
                                }
                            }else{
                                r.append(&mut b"</".to_vec());
                                r.append(&mut name.to_vec());
                                r.push(b'>');
                            }
                        }
                    }
                    ,Event::CData(c)=>{
                        r.append(&mut c.into_inner().to_vec());
                    }
                    ,Event::Text(c)=>{
                        r.append(&mut c.unescape().expect("Error!").as_bytes().to_vec());
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
