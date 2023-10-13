use std::{
    collections::HashMap,
    io::{BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    sync::Arc,
};

use wild_doc_script::IncludeAdaptor;

pub struct IncludeEmpty {}
impl IncludeEmpty {
    pub fn new() -> Self {
        Self {}
    }
}
impl IncludeAdaptor for IncludeEmpty {
    fn include(&mut self, _: &Path) -> Option<Arc<Vec<u8>>> {
        None
    }
}

pub struct IncludeRemote {
    stream: TcpStream,
    cache: HashMap<PathBuf, Arc<Vec<u8>>>,
}
impl IncludeRemote {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            cache: HashMap::default(),
        }
    }
}
impl IncludeAdaptor for IncludeRemote {
    fn include(&mut self, path: &Path) -> Option<Arc<Vec<u8>>> {
        if !self.cache.contains_key(path) {
            if let Some(path_str) = path.to_str() {
                if path_str.len() > 0 {
                    self.stream
                        .write(("include:".to_owned() + path_str).as_bytes())
                        .unwrap();
                    self.stream.write(&[0]).unwrap();
                    let mut reader = BufReader::new(&self.stream);

                    let mut exists: [u8; 1] = [0];
                    if let Ok(()) = reader.read_exact(&mut exists) {
                        let exists = u8::from_be_bytes(exists);
                        if exists == 1 {
                            let mut len: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
                            if let Ok(()) = reader.read_exact(&mut len) {
                                let len = u64::from_be_bytes(len) as usize;
                                let mut recv_response = Vec::<u8>::with_capacity(len);
                                unsafe {
                                    recv_response.set_len(len);
                                }
                                if let Ok(()) = reader.read_exact(recv_response.as_mut_slice()) {
                                    self.cache.insert(path.to_owned(), Arc::new(recv_response));
                                }
                            }
                        } else {
                            self.cache.insert(path.to_owned(), Arc::new(Vec::new()));
                        }
                    }
                }
            }
        }
        self.cache.get(path).cloned()
    }
}
