use std::{rc::Rc, cell::RefCell};

use quick_xml::{
    Reader
    ,events::Event
};
use semilattice_database::Database;

mod script;
use script::Script;

mod xml_util;

pub struct SemilatticeScript{
    database:Rc<RefCell<Database>>
}
impl SemilatticeScript{
    pub fn new(dir:&str)->Result<SemilatticeScript,std::io::Error>{
        Ok(SemilatticeScript{
            database:Rc::new(RefCell::new(Database::new(dir)?))
        })
    }

    
    pub fn exec(&mut self,qml:&str)->String{
        let mut reader=Reader::from_str(qml.trim());
        reader.expand_empty_elements(true);
        loop{
            match reader.read_event(){
                Ok(Event::Start(e))=>{
                    if e.name().as_ref()==b"ss"{
                        let mut script=Script::new(
                            self.database.clone()
                        );
                        return script.parse_xml(&mut reader);
                    }
                }
                ,_=>{}
            }
        }
    }
}
