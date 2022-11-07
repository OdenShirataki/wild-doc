use std::io::Read;

pub trait IncludeAdaptor{
    fn include(&self,path:&str)->String;
}
pub struct IncludeLocal{
    dir:String
}
impl IncludeLocal{
    pub fn new(dir:impl Into<String>)->Self{
        Self{
            dir:dir.into()
        }
    }
}
impl IncludeAdaptor for IncludeLocal{
    fn include(&self,path:&str)->String{
        if let Ok(mut f)=std::fs::File::open(&(self.dir.clone()+path)){
            let mut contents = String::new();
            let _=f.read_to_string(&mut contents);
            contents
        }else{
            "".to_string()
        }
    }
}