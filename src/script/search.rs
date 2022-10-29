use chrono::NaiveDateTime;
use quick_xml::{
    events::Event
    ,Reader
};
use semilattice_database::{
    Activity
    ,Condition
    ,CollectionRow
    ,search
};

use crate::xml_util::{self, XmlAttr};

use super::Script;

pub(super) fn make_conditions(script:&Script,attr:&XmlAttr,reader: &mut Reader<&[u8]>,scope: &mut v8::HandleScope)->Vec<Condition>{
    let mut conditions=condition_loop(script,reader,scope);

    let activity=attr.get("activity").map_or(
        Some(Activity::Active)
        ,|v|std::str::from_utf8(v).map_or(
            Some(Activity::Active)
            ,|v|if v=="all"{
                None
            }else if v=="inactive"{
                Some(Activity::Inactive)
            }else{
                Some(Activity::Active)
            }
        )
    );
    if let Some(activity)=activity{
        conditions.push(Condition::Activity(activity));
    }
    let term=attr.get("term").map_or(
        Some(search::Term::In(chrono::Local::now().timestamp()))
        ,|v|{
            std::str::from_utf8(v).map_or(
                Some(search::Term::In(chrono::Local::now().timestamp()))
                ,|v|{
                    if v=="all"{
                        None
                    }else{
                        let v:Vec<&str>=v.split("@").collect();
                        if v.len()==2{
                            NaiveDateTime::parse_from_str(v[1],"%Y-%m-%d %H:%M:%S").map_or(
                                Some(search::Term::In(chrono::Local::now().timestamp()))
                                ,|t|{
                                    match v[0]{
                                        "in"=>Some(search::Term::In(t.timestamp()))
                                        ,"future"=>Some(search::Term::Future(t.timestamp()))
                                        ,"past"=>Some(search::Term::Past(t.timestamp()))
                                        ,_ =>Some(search::Term::In(chrono::Local::now().timestamp()))
                                    }
                                }
                            )
                        }else{
                            Some(search::Term::In(chrono::Local::now().timestamp()))
                        }
                    }
                }
            )
        }
    );
    if let Some(term)=term{
        conditions.push(Condition::Term(term));
    }
    conditions
}

fn condition_loop(script:&Script,reader: &mut Reader<&[u8]>,scope: &mut v8::HandleScope)->Vec<Condition>{
    let mut conditions=Vec::new();
    loop{
        if let Ok(next)=reader.read_event(){
            match next{
                Event::Start(ref e)=>{
                    match e.name().as_ref(){
                        b"field"=>{
                            if let Some(c)=condition_field(xml_util::attr2hash_map(&e),scope){
                                conditions.push(c);
                            }
                            reader.read_to_end(e.name()).unwrap();
                        }
                        ,b"row"=>{
                            if let Some(c)=condition_row(xml_util::attr2hash_map(&e),scope){
                                conditions.push(c);
                            }
                            reader.read_to_end(e.name()).unwrap();
                        }
                        ,b"narrow"=>{
                            conditions.push(Condition::Narrow(condition_loop(script,reader,scope)));
                        }
                        ,b"wide"=>{
                            conditions.push(Condition::Wide(condition_loop(script,reader,scope)));
                        }
                        ,b"depend"=>{
                            let attrs=xml_util::attr2hash_map(e);
                            if let (
                                Some(row)
                                ,Some(collection_name)
                            )=(
                                attrs.get("row")
                                ,attrs.get("collection")
                            ){
                                if let (
                                    Ok(row)
                                    ,Ok(collection_name)
                                )=(
                                    std::str::from_utf8(row)
                                    ,std::str::from_utf8(collection_name)
                                ){
                                    let row=crate::eval_result(scope,row);
                                    let collection_name=crate::eval_result(scope,collection_name);

                                    if let (
                                        Ok(row)
                                        ,Some(collection_id)
                                    )=(
                                        row.parse::<u32>()
                                        ,script.database.clone().borrow().collection_id(&collection_name)
                                    ){
                                        let key=crate::eval_result(scope,if let Some(key)=attrs.get("key"){
                                            if let Ok(key)=std::str::from_utf8(key){
                                                key
                                            }else{
                                                ""
                                            }
                                        }else{
                                            ""
                                        });
                                        conditions.push(Condition::Depend(
                                            search::Depend::new(
                                                key,CollectionRow::new(
                                                    collection_id
                                                    ,row
                                                )
                                            )
                                        ));
                                    }
                                }
                            }
                        }
                        ,_=>{}
                    }
                }
                ,Event::End(e)=>{
                    match e.name().as_ref(){
                        b"ss:search"|b"narrow"|b"wide"=>{
                            break;
                        }
                        ,_=>{}
                    }
                }
                ,_=>{}
            }
        }
    }
    conditions
}
fn condition_row<'a>(e:XmlAttr,scope: &mut v8::HandleScope)->Option<Condition>{
    if let (
        Some(method)
        ,Some(value)
    )=(
        e.get("method")
        ,e.get("value")
    ){
        if let (
            Ok(method)
            ,Ok(value)
        )=(
            std::str::from_utf8(method)
            ,std::str::from_utf8(value)
        ){
            let method=crate::eval_result(scope,method);
            let value=crate::eval_result(scope,value);
            match &*method{
                "in"=>{
                    let mut v=Vec::<isize>::new();
                    for s in value.split(','){
                        if let Ok(i)=s.parse::<isize>(){
                            v.push(i);
                        }
                    }
                    if v.len()>0{
                        return Some(Condition::Row(search::Number::In(v)));
                    }
                }
                ,"min"=>{
                    if let Ok(v)=value.parse::<isize>(){
                        return Some(Condition::Row(search::Number::Min(v)));
                    }
                }
                ,"max"=>{
                    if let Ok(v)=value.parse::<isize>(){
                        return Some(Condition::Row(search::Number::Max(v)));
                    }
                }
                ,"range"=>{
                    let s: Vec<&str>=value.split("..").collect();
                    if s.len()==2{
                        if let (
                            Ok(min),Ok(max)
                        )=(
                            s[0].parse::<u32>()
                            ,s[1].parse::<u32>()
                        ){
                            return Some(Condition::Row(search::Number::Range((min as isize)..=(max as isize))));
                        }
                        
                    }
                }
                ,_ =>{}
            }
        }
    }
    None
}
fn condition_field<'a>(e:XmlAttr,scope: &mut v8::HandleScope)->Option<Condition>{
    if let (
        Some(name)
        ,Some(method)
        ,Some(value)
    )=(
        e.get("name")
        ,e.get("method")
        ,e.get("value")
    ){
        if let (
            Ok(name)
            ,Ok(method)
            ,Ok(value)
        )=(
            std::str::from_utf8(name)
            ,std::str::from_utf8(method)
            ,std::str::from_utf8(value)
        ){
            let name=crate::eval_result(scope,name);
            let method=crate::eval_result(scope,method);
            let value=crate::eval_result(scope,value);
            
            let method_pair:Vec<&str>=method.split('!').collect();
            let len=method_pair.len();
            let i=len-1;
            //let not=len>1;

            if let Some(method)=match method_pair[i]{
                "match"=>Some(search::Field::Match(value.as_bytes().to_vec()))
                ,"min"=>Some(search::Field::Min(value.as_bytes().to_vec()))
                ,"max"=>Some(search::Field::Max(value.as_bytes().to_vec()))
                ,"partial"=>Some(search::Field::Partial(value.to_string()))
                ,"forward"=>Some(search::Field::Forward(value.to_string()))
                ,"backward"=>Some(search::Field::Backward(value.to_string()))
                ,"range"=>{
                    let s: Vec<&str>=value.split("..").collect();
                    if s.len()==2{
                        Some(search::Field::Range(s[0].as_bytes().to_vec(),s[1].as_bytes().to_vec()))
                    }else{
                        None
                    }
                }
                ,_=>None
            }{
                return Some(
                    Condition::Field(name.to_string(),method)
                );
            }
        }
    }
    None
}