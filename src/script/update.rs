use std::collections::HashMap;

use chrono::TimeZone;
use quick_xml::{
    Reader
    ,events::Event
};
use semilattice_database::{
    Database
    ,Session
    ,Record
    ,Pend
    ,Activity
    ,Term
    ,KeyValue
    ,Depends
};

use crate::xml_util;

pub fn make_update_struct(
    database:&mut Database
    ,session:&mut Session
    ,reader:&mut Reader<&[u8]>
)->Vec<Record>{
    let mut updates=Vec::new();
    loop{
        match reader.read_event(){
            Ok(Event::Start(e))=>{
                if e.name().as_ref()==b"collection"{
                    if let Ok(Some(collection_name))=e.try_get_attribute("name"){
                        if let Ok(collection_name)=std::str::from_utf8(&collection_name.value){
                            let mut pends=Vec::new();
                            let mut fields=HashMap::new();
                            loop{
                                match reader.read_event(){
                                    Ok(Event::Start(e))=>{
                                        if e.name().as_ref()==b"field"{
                                            if let Ok(Some(field_name))=e.try_get_attribute(b"name"){
                                                if let Ok(field_name)=std::str::from_utf8(&field_name.value){
                                                    fields.insert(field_name.to_string(),xml_util::text_content(reader,e.name()));
                                                }
                                            }
                                        }else if e.name().as_ref()==b"pends"{
                                            let pends_tmp=make_update_struct(database,session,&mut Reader::from_str(&xml_util::inner(reader)));
                                            if let Ok(Some(key))=e.try_get_attribute("key"){
                                                if let Ok(key)=std::str::from_utf8(&key.value){
                                                    pends.push(Pend::new(key,pends_tmp));
                                                }
                                            }
                                        }
                                    }
                                    ,Ok(Event::End(e))=>{
                                        if e.name().as_ref()==b"collection"{
                                            break;
                                        }
                                    }
                                    ,_=>{}
                                }
                            }
                            let attr=xml_util::attr2hash_map(&e);

                            let row=if let Some(row)=attr.get("row"){
                                std::str::from_utf8(row).unwrap_or("0").parse().unwrap_or(0)
                            }else{
                                0
                            };
                            let activity=if let Some(v)=attr.get("activity"){
                                match std::str::from_utf8(v){
                                    Ok("inactive")=>Activity::Inactive
                                    ,Ok("0")=>Activity::Inactive
                                    ,_=>Activity::Active
                                }
                            }else{
                                Activity::Active
                            };
                            let term_begin=if let Some(v)=attr.get("term_begin"){
                                if let Some(t)=std::str::from_utf8(v).map_or(
                                    None,|v|chrono::Local.datetime_from_str(v,"%Y-%m-%d %H:%M:%S").map_or(None,|v|Some(v.timestamp()))
                                ){
                                    Term::Overwrite(t)
                                }else{
                                    Term::Defalut
                                }
                                
                            }else{
                                Term::Defalut
                            };
                            let term_end=if let Some(v)=attr.get("term_end"){
                                if let Some(t)=std::str::from_utf8(v).map_or(
                                    None,|v|chrono::Local.datetime_from_str(v,"%Y-%m-%d %H:%M:%S").map_or(None,|v|Some(v.timestamp()))
                                ){
                                    Term::Overwrite(t)
                                }else{
                                    Term::Defalut
                                }
                            }else{
                                Term::Defalut
                            };
                            /*
                            let is_delete=if let Some(v)=attr.get("delete"){
                                if let Ok(v)=std::str::from_utf8(v){
                                    v=="1"
                                }else{
                                    false
                                }
                            }else{
                                false
                            }; */
                            let collection_id=database.collection_id_or_create(collection_name).unwrap();
                            let mut f=Vec::new();
                            for (key,value) in fields{
                                f.push(KeyValue::new(key,value))
                            }
                            if row==0{
                                updates.push(Record::New{
                                    collection_id
                                    ,activity
                                    ,term_begin
                                    ,term_end
                                    ,fields:f
                                    ,depends:Depends::Default
                                    ,pends
                                });
                            }
                        }
                    }
                }
            }
            ,Ok(Event::End(e))=>{
                if e.name().as_ref()==b"ss:update"{
                    break;
                }
            }
            ,Ok(Event::Eof)=>{
                break;
            }
            ,_ => {}
        }
    }
    updates
}