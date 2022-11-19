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

use crate::{xml_util, IncludeAdaptor};

use super::Script;

pub(super) fn case<T:IncludeAdaptor>(script:&mut Script,e:&BytesStart,xml_str:&str,scope: &mut v8::HandleScope,include_adaptor:&mut T)->Result<String,std::io::Error>{
    let mut r=String::new();
    let attr=xml_util::attr2hash_map(&e);
    let cmp_value=crate::attr_parse_or_static(scope,&attr,"value");
    if cmp_value!=""{
        let mut event_reader=Reader::from_str(&xml_str.trim());
        loop{
            match event_reader.read_event(){
                Ok(Event::Start(e))=>{
                    if e.name().as_ref()==b"wd:case"{
                        'case:loop{
                            if let Ok(next)=event_reader.read_event(){
                                match next{
                                    Event::Start(ref e)=>{
                                        match e.name().as_ref(){
                                            b"wd:when"=>{
                                                let attr=xml_util::attr2hash_map(&e);
                                                let wv=crate::attr_parse_or_static(scope,&attr,"value");
                                                if wv==cmp_value{
                                                    let xml_str=xml_util::outer(&next,&mut event_reader);
                                                    let mut event_reader_inner=Reader::from_str(&xml_str.trim());
                                                    loop{
                                                        match event_reader_inner.read_event(){
                                                            Ok(Event::Start(e))=>{
                                                                if e.name().as_ref()==b"wd:when"{
                                                                    r+=&script.parse(scope,&mut event_reader_inner,"",include_adaptor)?;
                                                                    break 'case;
                                                                }
                                                            }
                                                            ,_=>{}
                                                        }
                                                    }
                                                }
                                            }
                                            ,b"wd:else"=>{
                                                let xml_str=xml_util::outer(&next,&mut event_reader);
                                                let mut event_reader_inner=Reader::from_str(&xml_str.trim());
                                                loop{
                                                    match event_reader_inner.read_event(){
                                                        Ok(Event::Start(e))=>{
                                                            if e.name().as_ref()==b"wd:else"{
                                                                r+=&script.parse(scope,&mut event_reader_inner,"",include_adaptor)?;
                                                                break;
                                                            }
                                                        }
                                                        ,_=>{}
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
    Ok(r)
}
pub(super) fn r#for<T:IncludeAdaptor>(script:&mut Script,e:&BytesStart,xml_str:&str,scope: &mut v8::HandleScope,include_adaptor:&mut T)->Result<String,std::io::Error>{
    let mut r=String::new();
    let attr=xml_util::attr2hash_map(&e);
    let var=crate::attr_parse_or_static(scope,&attr,"var");
    if var!=""{
        if let Some(arr)=attr.get("wd:in"){
            if let Ok(arr)=std::str::from_utf8(arr){
                if let Some(rs)=v8::String::new(scope,&arr)
                    .and_then(|code|v8::Script::compile(scope,code, None))
                    .and_then(|code|code.run(scope))
                    .and_then(|v|v8::Local::<v8::Array>::try_from(v).ok())
                {
                    let length=rs.length();
                    for i in 0..length {
                        let mut ev=Reader::from_str(&xml_str);
                        loop{
                            match ev.read_event(){
                                Ok(Event::Start(e))=>{
                                    if e.name().as_ref()==b"wd:for"{
                                        v8::String::new(
                                            scope
                                            ,&("wd.stack.push({".to_owned()+&var.to_string()+":"+arr+"["+&i.to_string()+"]"+&(
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
                                        r+=&script.parse(scope,&mut ev,"wd:for",include_adaptor)?;
                                        v8::String::new(scope,"wd.stack.pop()")
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
    }
    Ok(r)
}