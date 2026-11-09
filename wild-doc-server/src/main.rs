mod include;

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex},
    thread,
};

use anyhow::Result;
use serde::Deserialize;

use wild_doc::{DataOption, WildDoc};

use include::{IncludeEmpty, IncludeRemote};
use wild_doc_script::{IncludeAdaptor, serde_json};

#[derive(Deserialize)]
struct Config {
    server: Option<ConfigServer>,
}
#[derive(Deserialize)]
struct ConfigServer {
    path: Option<String>,
    bind_addr: Option<String>,
    port: Option<String>,
    delete_dir_on_start: Option<String>,
    collection: Option<HashMap<String, DataOption>>,
    relation_reserve_unit: Option<u32>,
}

struct WildDocWrapper<DI: IncludeAdaptor + Send> {
    inner: WildDoc<DI>,
}
unsafe impl<DI: IncludeAdaptor + Send> Sync for WildDocWrapper<DI> {}
unsafe impl<DI: IncludeAdaptor + Send> Send for WildDocWrapper<DI> {}

impl<DI: IncludeAdaptor + Send> Deref for WildDocWrapper<DI> {
    type Target = WildDoc<DI>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl<DI: IncludeAdaptor + Send> DerefMut for WildDocWrapper<DI> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

fn main() {
    if let Ok(mut f) = std::fs::File::open("wild-doc-server.toml") {
        let mut toml = String::new();
        if f.read_to_string(&mut toml).is_ok() {
            let config: Result<Config, toml::de::Error> = toml::from_str(&toml);
            if let Ok(config) = config {
                if let Some(config) = config.server {
                    if let (Some(dir), Some(bind_addr), Some(port)) =
                        (config.path, config.bind_addr, config.port)
                    {
                        if let Some(delete_dir_on_start) = config.delete_dir_on_start {
                            if delete_dir_on_start == "1" && std::path::Path::new(&dir).exists() {
                                std::fs::remove_dir_all(&dir).unwrap();
                            }
                        }

                        let wild_docs = Arc::new(Mutex::new(HashMap::new()));
                        let collection = config.collection;
                        let relation_reserve_unit = config.relation_reserve_unit.unwrap_or(1);
                        let listener = TcpListener::bind(&(bind_addr + ":" + port.as_str()))
                            .expect("Error. failed to bind.");
                        for streams in listener.incoming() {
                            match streams {
                                Err(e) => {
                                    eprintln!("error: {}", e)
                                }
                                Ok(stream) => {
                                    let dir = dir.clone();
                                    let wild_docs = Arc::clone(&wild_docs);
                                    let collection = collection.clone();
                                    // read the database name in the thread, so that a client
                                    // that sends nothing does not block other clients.
                                    thread::spawn(move || {
                                        let wd = {
                                            let mut dbname = Vec::new();
                                            let mut tcp_reader = BufReader::new(&stream);
                                            match tcp_reader.read_until(0, &mut dbname) {
                                                Ok(nbytes) if nbytes > 0 => {}
                                                _ => {
                                                    println!("recv 0 bytes");
                                                    return;
                                                }
                                            }
                                            dbname.pop();
                                            let Some(dbname) = std::str::from_utf8(&dbname)
                                                .ok()
                                                .filter(|name| is_valid_dbname(name))
                                            else {
                                                eprintln!("invalid database name");
                                                return;
                                            };
                                            let dir = dir + dbname + "/";
                                            let mut wild_docs = wild_docs.lock().unwrap();
                                            let wd =
                                                wild_docs.entry(dir).or_insert_with_key(|dir| {
                                                    if !std::path::Path::new(dir).exists() {
                                                        std::fs::create_dir_all(dir).unwrap();
                                                    }
                                                    Arc::new(Mutex::new(WildDocWrapper {
                                                        inner: WildDoc::new(
                                                            dir,
                                                            IncludeEmpty::new(),
                                                            collection,
                                                            relation_reserve_unit,
                                                        ),
                                                    }))
                                                });
                                            Arc::clone(wd)
                                        };
                                        handler(stream, wd).unwrap_or_else(|error| {
                                            eprintln!("handler {:?}", error)
                                        });
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A database name becomes a directory name, so it must not point outside of
/// the data directory.
fn is_valid_dbname(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn handler<DI: IncludeAdaptor + Send>(
    mut stream: TcpStream,
    wd: Arc<Mutex<WildDocWrapper<DI>>>,
) -> Result<()> {
    stream.write_all(&[0])?;

    let mut writer = stream.try_clone().unwrap();
    let mut tcp_reader = BufReader::new(&stream);
    loop {
        let mut input_json = Vec::new();
        let nbytes = tcp_reader.read_until(0, &mut input_json)?;
        if nbytes == 0 {
            break;
        }
        input_json.remove(input_json.len() - 1);

        let mut xml = Vec::new();
        let nbytes = tcp_reader.read_until(0, &mut xml)?;
        if nbytes == 0 {
            break;
        }
        xml.remove(xml.len() - 1);

        let ret = wd.clone().lock().unwrap().run_with_include_adaptor(
            &xml,
            &input_json,
            IncludeRemote::new(stream.try_clone().unwrap()),
        );
        match ret {
            Ok(r) => {
                let body = r.body();
                let len = body.len() as u64;
                writer.write_all(&[0])?;
                writer.write_all(&len.to_be_bytes())?;
                writer.write_all(body)?;
                if let Some(response) = r.options().get(&Arc::new("response".into())) {
                    if let Ok(json) = serde_json::to_string(response) {
                        writer.write_all(json.as_bytes())?;
                    } else {
                        writer.write_all(b"")?;
                    }
                } else {
                    writer.write_all(b"")?;
                }
                writer.write_all(&[0])?;
            }
            Err(e) => {
                let body = e.to_string();
                let len = body.len() as u64;
                writer.write_all(&[0])?;
                writer.write_all(&len.to_be_bytes())?;
                writer.write_all(body.as_bytes())?;
                writer.write_all(b"")?;
                writer.write_all(&[0])?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_valid_dbname;

    #[test]
    fn dbname_cannot_escape_the_data_directory() {
        for name in ["db", "my-db_1", "a.b"] {
            assert!(is_valid_dbname(name), "{name}");
        }
        for name in ["", ".", "..", "../x", "a/b", "C:", "a b", "x\\y"] {
            assert!(!is_valid_dbname(name), "{name}");
        }
    }
}
