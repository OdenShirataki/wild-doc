use std::{
    convert::TryFrom
};
use quick_xml::{
    Reader
    ,events::{
        Event
        ,BytesStart
    }
};

use crate::xml_util;

use super::Script;

pub(super) fn print(e:&BytesStart,scope: &mut v8::HandleScope)->String{
    let mut r=String::new();
    if let Ok(Some(value))=e.try_get_attribute(b"value"){
        if let Ok(code)=std::str::from_utf8(&value.value){
            if let Some(result)=v8::String::new(scope,&code)
                .and_then(|code|v8::Script::compile(scope, code, None))
                .and_then(|v|v.run(scope))
                .and_then(|v|v.to_string(scope))
            {
                r.push_str(&result.to_rust_string_lossy(scope));
            }
        }
    }
    r
}
pub(super) fn case(script:&mut Script,e:&BytesStart,xml_str:&str,scope: &mut v8::HandleScope)->String{
    let mut r=String::new();
    if let Ok(Some(value))=e.try_get_attribute(b"value"){
        if let Some(cmp_value)=std::str::from_utf8(&value.value).ok()
            .and_then(|code|v8::String::new(scope,code))
            .and_then(|code|v8::Script::compile(scope, code, None))
            .and_then(|v|v.run(scope))
        {
            let mut event_reader=Reader::from_str(&xml_str.trim());
            event_reader.expand_empty_elements(true);
            loop{
                match event_reader.read_event(){
                    Ok(Event::Start(e))=>{
                        if e.name().as_ref()==b"ss:case"{
                            'case:loop{
                                if let Ok(next)=event_reader.read_event(){
                                    match next{
                                        Event::Start(ref e)=>{
                                            match e.name().as_ref(){
                                                b"ss:else"=>{
                                                    let xml_str=xml_util::outer(&next,&mut event_reader);
                                                    let mut event_reader_inner=Reader::from_str(&xml_str.trim());
                                                    event_reader_inner.expand_empty_elements(true);
                                                    loop{
                                                        match event_reader_inner.read_event(){
                                                            Ok(Event::Start(e))=>{
                                                                if e.name().as_ref()==b"ss:else"{
                                                                    r+=&script.parse(scope,&mut event_reader_inner,"");
                                                                    break;
                                                                }
                                                            }
                                                            ,_=>{}
                                                        }
                                                    }
                                                }
                                                ,b"ss:when"=>{
                                                    if let Ok(Some(value))=e.try_get_attribute(b"value"){
                                                        if let Some(wv)=std::str::from_utf8(&value.value).ok()
                                                            .and_then(|code|v8::String::new(scope,code))
                                                            .and_then(|code|v8::Script::compile(scope, code, None))
                                                            .and_then(|v|v.run(scope))
                                                        {
                                                            if wv==cmp_value{
                                                                let xml_str=xml_util::outer(&next,&mut event_reader);
                                                                let mut event_reader_inner=Reader::from_str(&xml_str.trim());
                                                                event_reader_inner.expand_empty_elements(true);
                                                                loop{
                                                                    match event_reader_inner.read_event(){
                                                                        Ok(Event::Start(e))=>{
                                                                            if e.name().as_ref()==b"ss:when"{
                                                                                r+=&script.parse(scope,&mut event_reader_inner,"");
                                                                                break 'case;
                                                                            }
                                                                        }
                                                                        ,_=>{}
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                ,_=>{}
                                            }
                                        }
                                        ,Event::Eof=>{
                                            break;
                                        }
                                        ,_=>{}
                                    }
                                }
                            }
                            break;
                        }
                    }
                    ,_=>{}
                }
            }
        }
    }
    r
}
pub(super) fn r#for(script:&mut Script,e:&BytesStart,xml_str:&str,scope: &mut v8::HandleScope)->String{
    let mut r=String::new();
    if let (Ok(Some(var)),Ok(Some(arr)))=(e.try_get_attribute(b"var"),e.try_get_attribute(b"in")){
        if let (Ok(arr),Ok(var))=(std::str::from_utf8(&arr.value),std::str::from_utf8(&var.value)){
            if let Some(rs)=v8::String::new(scope,&arr)
                .and_then(|code|v8::Script::compile(scope,code, None))
                .and_then(|code|code.run(scope))
                .and_then(|v|v8::Local::<v8::Array>::try_from(v).ok())
            {
                let length=rs.length();
                for i in 0..length {
                    let mut ev=Reader::from_str(&xml_str);
                    ev.expand_empty_elements(true);
                    loop{
                        match ev.read_event(){
                            Ok(Event::Start(e))=>{
                                if e.name().as_ref()==b"ss:for"{
                                    v8::String::new(
                                        scope
                                        ,&("ss.stack.push({".to_owned()+&var.to_string()+":"+arr+"["+&i.to_string()+"]"+&(
                                        if let Ok(Some(index))=e.try_get_attribute(b"index"){
                                            std::str::from_utf8(&index.value).map_or("".to_string(),|v|",".to_owned()+v+":"+&i.to_string())
                                        }else{
                                            "".to_owned()
                                        }
                                        )+"})")
                                    )
                                        .and_then(|code|v8::Script::compile(scope, code, None))
                                        .and_then(|v|v.run(scope))
                                    ;
                                    r+=&script.parse(scope,&mut ev,"");
                                    v8::String::new(scope,"ss.stack.pop()")
                                        .and_then(|code|v8::Script::compile(scope, code, None))
                                        .and_then(|v|v.run(scope))
                                    ;
                                    break;
                                }
                            }
                            ,_=>{}
                        }
                    }
                }
            }
        }
    }
    r
}

/*
pub(super) fn include(session:&mut Session,e:&BytesStart,scope: &mut v8::HandleScope)->String{
    let mut r=String::new();
    if let Ok(Some(src))=e.try_get_attribute(b"src"){
        if let Some(src)=std::str::from_utf8(&src.value).ok().and_then(|src|v8::String::new(scope,&src))
            .and_then(|code|v8::Script::compile(scope, code, None))
            .and_then(|v|v.run(scope))
            .and_then(|v|v.to_string(scope))
        {
            if let Ok(mut file)=OpenOptions::new()
                .read(true)
                .write(false)
                .create(false)
                .open("d:/data/script/".to_owned()+&src.to_rust_string_lossy(scope))
            {
                let mut contents = String::new();
                if let Ok(_)=file.read_to_string(&mut contents){
                    let xml_str="<ss:select xmlns:ss=\"ss\">".to_owned()+&contents+"</ss:select>";
                    let mut event_reader_inner=Reader::from_str(&xml_str);
                    event_reader_inner.expand_empty_elements(true);
                    loop{
                        match event_reader_inner.read_event(){
                            Ok(Event::Start(e))=>{
                                if e.name().as_ref()==b"ss:select"{
                                    r+=&super::Script::parse(session,scope,&mut event_reader_inner);
                                    break;
                                }
                            }
                            ,_=>{}
                        }
                    }
                }
            }
        }
    }
    r
}
*/

pub(super) fn html(name:&[u8],e:&BytesStart,scope: &mut v8::HandleScope)->String{
    let mut r=String::new();
    if !name.starts_with(b"ss:"){
        let mut html_attr="".to_string();
        for attr in e.attributes(){
            if let Ok(attr)=attr{
                if let Ok(attr_key)=std::str::from_utf8(attr.key.as_ref()){
                    let is_ss=attr_key.starts_with("ss:");
                    let attr_key=if is_ss{
                        attr_key.split_at(4).1
                    }else{
                        attr_key
                    };
                    html_attr.push(' ');
                    html_attr.push_str(attr_key);
                    html_attr.push_str("=\"");
                    
                    if let Ok(value)=std::str::from_utf8(&attr.value){
                        if is_ss{
                            if let Some(result)=v8::String::new(scope,value)
                                .and_then(|code|v8::Script::compile(scope, code, None))
                                .and_then(|v|v.run(scope))
                                .and_then(|result|result.to_string(scope))
                            {
                                html_attr.push_str(&result.to_rust_string_lossy(scope));
                            }
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
        r.push('<');
        r.push_str(std::str::from_utf8(name).unwrap_or(""));
        r.push_str(&html_attr);
        r.push('>');
    }
    r
}
