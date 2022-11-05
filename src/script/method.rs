use std::{convert::TryFrom, sync::{Arc, RwLock}};

use semilattice_database::Database;

pub fn v(
    scope: &mut v8::HandleScope
    ,args: v8::FunctionCallbackArguments
    ,mut rv: v8::ReturnValue
){
    if let (
        Some(v8str_var)
        ,Some(v8str_ss)
        ,Some(v8str_stack)
    )=(
        args.get(0).to_string(scope)
        ,v8::String::new(scope,"ss")
        ,v8::String::new(scope,"stack")
    ){
        let context=scope.get_current_context();
        let global=context.global(scope);
        if let Some(ss)=global.get(scope,v8str_ss.into()){
            if let Ok(ss)=v8::Local::<v8::Object>::try_from(ss){
                if let Some(stack)=ss.get(scope,v8str_stack.into()){
                    if let Ok(stack)=v8::Local::<v8::Array>::try_from(stack){
                        for i in (0..stack.length()).rev(){
                            if let Some(cs)=stack.get_index(scope,i){
                                if let Ok(cs)=v8::Local::<v8::Object>::try_from(cs){
                                    if let Some(v)=cs.get(scope,v8str_var.into()){
                                        if v.is_undefined()==false{
                                            rv.set(v);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
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
