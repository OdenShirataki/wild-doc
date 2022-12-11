use std::{collections::HashMap, sync::{Arc, RwLock}};

use deno_runtime::{worker::MainWorker, deno_napi::v8};
use quick_xml::events::BytesStart;
use semilattice_database::{Order, OrderKey, Condition, Database, Session};

use crate::xml_util;

use super::Script;

struct ResultStateWidthSession{
    database:Arc<RwLock<Database>>
    ,session:Arc<RwLock<Session>>
}

pub fn field(
    scope: &mut v8::HandleScope
    ,args: v8::FunctionCallbackArguments
    ,mut rv: v8::ReturnValue
){
    let this = args.this();

    if let (
        Some(collection_id)
        ,Some(row)
    )=(
        v8::String::new(scope,"collection_id").and_then(|s|this.get(scope,s.into())).and_then(|s|s.to_string(scope))
        ,v8::String::new(scope,"row").and_then(|s|this.get(scope,s.into())).and_then(|s|s.to_string(scope))
    ){
        if let (
            Ok(collection_id)
            ,Ok(row)
            ,Some(field_name)
        )=(
            collection_id.to_rust_string_lossy(scope).parse::<i32>()
            ,row.to_rust_string_lossy(scope).parse::<u32>()
            ,args.get(0).to_string(scope)
        ){
            let field_name=field_name.to_rust_string_lossy(scope);

            if let Some(database)=scope.get_slot::<Arc<RwLock<Database>>>(){
                if let Some(data)=database.clone().read().unwrap().collection(collection_id){
                    let main_row=row as u32;
                    match field_name.as_str(){
                        "_activity"=>{
                            rv.set(v8::Integer::new(scope,data.activity(main_row) as i32).into());
                        }
                        ,"_term_begin"=>{
                            rv.set(crate::eval(scope,&data.term_begin(main_row).to_string()).unwrap());
                        }
                        ,"_term_end"=>{
                            rv.set(crate::eval(scope,&data.term_end(main_row).to_string()).unwrap());
                        }
                        ,"_last_updated"=>{
                            rv.set(crate::eval(scope,&data.last_updated(main_row).to_string()).unwrap());
                        }
                        ,"_uuid"=>{
                        if let Some(str)=v8::String::new(scope,&data.uuid_str(main_row)){
                            rv.set(str.into());
                        }
                        }
                        ,_=>{
                            if let Some(str)=v8::String::new(scope,std::str::from_utf8(data.field_bytes(main_row,&field_name)).unwrap()){
                                rv.set(str.into());
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn session_field(
    scope: &mut v8::HandleScope
    ,args: v8::FunctionCallbackArguments
    ,mut rv: v8::ReturnValue
){
    let this = args.this();

    if let (
        Some(collection_id)
        ,Some(row)
    )=(
        v8::String::new(scope,"collection_id").and_then(|s|this.get(scope,s.into())).and_then(|s|s.to_string(scope))
        ,v8::String::new(scope,"row").and_then(|s|this.get(scope,s.into())).and_then(|s|s.to_string(scope))
    ){
        if let (
            Ok(collection_id)
            ,Ok(row)
            ,Some(field_name)
        )=(
            collection_id.to_rust_string_lossy(scope).parse::<i32>()
            ,row.to_rust_string_lossy(scope).parse::<i64>()
            ,args.get(0).to_string(scope)
        ){
            let field_name=field_name.to_rust_string_lossy(scope);

            if let Some(state)=scope.get_slot::<ResultStateWidthSession>(){
                if let Ok(session)=state.session.clone().read(){
                    if let Some(data)=state.database.clone().read().unwrap().collection(collection_id){
                        let main_row=if row<0{
                            -row as u32
                        }else{
                            row as u32
                        };
                        match field_name.as_str(){
                            "_activity"=>{
                                rv.set(v8::Integer::new(scope,data.activity(main_row) as i32).into());
                            }
                            ,"_term_begin"=>{
                                rv.set(crate::eval(scope,&data.term_begin(main_row).to_string()).unwrap());
                            }
                            ,"_term_end"=>{
                                rv.set(crate::eval(scope,&data.term_end(main_row).to_string()).unwrap());
                            }
                            ,"_last_updated"=>{
                                rv.set(crate::eval(scope,&data.last_updated(main_row).to_string()).unwrap());
                            }
                            ,"_uuid"=>{
                            if let Some(str)=v8::String::new(scope,&data.uuid_str(main_row)){
                                rv.set(str.into());
                            }
                            }
                            ,_=>{
                                let bytes=session.collection_field_bytes(&data,row,&field_name);
                                if let Some(str)=v8::String::new(scope,std::str::from_utf8(&bytes).unwrap()){
                                    rv.set(str.into());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn make_order(sort:&str)->Vec<Order>{
    let mut orders=vec![];
    if sort.len()>0{
        for o in sort.trim().split(","){
            let o=o.trim();
            let is_desc=o.ends_with(" DESC");
            let o_split: Vec<&str>=o.split(" ").collect();
            let field=o_split[0];
            let order_key=if field.starts_with("field."){
                if let Some(field_name)=field.strip_prefix("field."){
                    Some(OrderKey::Field(field_name.to_owned()))
                }else{
                    None
                }
            }else{
                match field{
                    "serial"=>Some(OrderKey::Serial)
                    ,"row"=>Some(OrderKey::Row)
                    ,"term_begin"=>Some(OrderKey::TermBegin)
                    ,"term_end"=>Some(OrderKey::TermEnd)
                    ,"last_update"=>Some(OrderKey::LastUpdated)
                    ,_=>None
                }
            };
            if let Some(order_key)=order_key{
                orders.push(
                    if is_desc{
                        Order::Desc(order_key)
                    }else{
                        Order::Asc(order_key)
                    }
                );
            }
        }
    }
    orders
}

pub(super) fn result(
    script:&Script
    ,worker: &mut MainWorker
    ,e: &BytesStart
    ,search_map: &HashMap<String, (i32, Vec<Condition>)>
){
    let attr=xml_util::attr2hash_map(&e);
    let search=crate::attr_parse_or_static(worker,&attr,"search");
    let var=crate::attr_parse_or_static(worker,&attr,"var");
 
    if search!="" && var!=""{
        if let Some((collection_id,conditions))=search_map.get(&search){
            let collection_id=*collection_id;
            let orders=make_order(&crate::attr_parse_or_static(worker,&attr,"sort"));

            let mut session_maybe_has_collection=None;
            for i in (0..script.sessions.len()).rev(){
                if let Some(_)=script.sessions[i].clone().read().unwrap().temporary_collection(collection_id){
                    session_maybe_has_collection=Some(script.sessions[i].clone());
                    break;
                }
            }
            
            if let Some(ref session)=session_maybe_has_collection{
                worker.js_runtime.v8_isolate().set_slot(ResultStateWidthSession{
                    database:script.database.clone()
                    ,session:session.clone()
                });
            }else{
                worker.js_runtime.v8_isolate().set_slot(script.database.clone());
            }
            

            let scope=&mut worker.js_runtime.handle_scope();
            let context=scope.get_current_context();
            let scope=&mut v8::ContextScope::new(scope,context);

            if let (
                Some(str_collection_id)
                ,Some(v8str_session_key)
                ,Some(v8str_func_field)
                ,Some(v8str_row)
                ,Some(v8str_wd)
                ,Some(v8str_stack)
                ,Some(var)
            )=(
                v8::String::new(scope,"collection_id")
                ,v8::String::new(scope,"session_key")
                ,v8::String::new(scope,"field")
                ,v8::String::new(scope,"row")
                ,v8::String::new(scope,"wd")
                ,v8::String::new(scope,"stack")
                ,v8::String::new(scope,&var)
            ){
                let context=scope.get_current_context();
                let global=context.global(scope);
                if let Some(wd)=global.get(scope,v8str_wd.into()){
                    if let Ok(wd)=v8::Local::<v8::Object>::try_from(wd){
                        if let Some(stack)=wd.get(scope,v8str_stack.into()){
                            if let Ok(stack)=v8::Local::<v8::Array>::try_from(stack){
                                let obj=v8::Object::new(scope);
                                let return_obj=v8::Array::new(scope,0);

                                if let Some(session)=session_maybe_has_collection{
                                    if let Some(v8func_field)=v8::Function::new(scope,session_field){
                                        let rowset=session.clone().read().unwrap().search(collection_id,conditions).result(&script.database.clone().read().unwrap());
                                        //TODO:セッションデータのソート
                                        let mut i=0;
                                        for d in rowset{
                                            let obj=v8::Object::new(scope);
                                            let row=v8::BigInt::new_from_i64(scope, d as i64);
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
                                }else{
                                    if let Some(collection)=script.database.clone().read().unwrap().collection(collection_id){
                                        if let Some(v8func_field)=v8::Function::new(scope,field){
                                            let mut search=script.database.clone().read().unwrap().search(collection);
                                            for c in conditions{
                                                search=search.search(c.clone());
                                            }
                                            
                                            let rowset=script.database.clone().read().unwrap().result(&search);
                                            let rows=if orders.len()>0{
                                                collection.sort(rowset,orders)
                                            }else{
                                                rowset.into_iter().collect()
                                            };
                                            let mut i=0;
                                            for d in rows{
                                                let obj=v8::Object::new(scope);
                                                let row=v8::BigInt::new_from_i64(scope, d as i64);
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