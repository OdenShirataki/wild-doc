mod include;

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
};

use serde::Deserialize;

use wild_doc::{anyhow::Result, WildDoc};

use include::{IncludeEmpty, IncludeRemote};

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
}

fn main() {
    if let Ok(mut f) = std::fs::File::open("wild-doc-server.toml") {
        let mut toml = String::new();
        if let Ok(_) = f.read_to_string(&mut toml) {
            let config: Result<Config, toml::de::Error> = toml::from_str(&toml);
            if let Ok(config) = config {
                if let Some(config) = config.server {
                    if let (Some(dir), Some(bind_addr), Some(port)) =
                        (config.path, config.bind_addr, config.port)
                    {
                        if let Some(delete_dir_on_start) = config.delete_dir_on_start {
                            if delete_dir_on_start == "1" {
                                if std::path::Path::new(&dir).exists() {
                                    std::fs::remove_dir_all(&dir).unwrap();
                                }
                            }
                        }

                        let mut wild_docs = HashMap::new();
                        let listener = TcpListener::bind(&(bind_addr + ":" + port.as_str()))
                            .expect("Error. failed to bind.");
                        for streams in listener.incoming() {
                            match streams {
                                Err(e) => {
                                    eprintln!("error: {}", e)
                                }
                                Ok(stream) => {
                                    let mut dbname = Vec::new();
                                    let mut tcp_reader = BufReader::new(&stream);
                                    let nbytes = tcp_reader.read_until(0, &mut dbname).unwrap();
                                    if nbytes > 0 {
                                        dbname.remove(dbname.len() - 1);
                                        if let Ok(dbname) = std::str::from_utf8(&dbname) {
                                            let dir = dir.to_owned() + dbname + "/";
                                            let wd =
                                                wild_docs.entry(dir).or_insert_with_key(|dir| {
                                                    if !std::path::Path::new(dir).exists() {
                                                        std::fs::create_dir_all(dir).unwrap();
                                                    }
                                                    Arc::new(Mutex::new(
                                                        WildDoc::new(
                                                            dir,
                                                            Box::new(IncludeEmpty::new()),
                                                        )
                                                        .unwrap(),
                                                    ))
                                                });
                                            let wd = Arc::clone(&wd);
                                            thread::spawn(move || {
                                                handler(stream, wd).unwrap_or_else(|error| {
                                                    eprintln!("handler {:?}", error)
                                                });
                                            });
                                        }
                                    } else {
                                        println!("recv 0 bytes");
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

fn handler(mut stream: TcpStream, wd: Arc<Mutex<WildDoc>>) -> Result<()> {
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

        let ret = wd.clone().lock().unwrap().run_specify_include_adaptor(
            &xml,
            &input_json,
            Box::new(IncludeRemote::new(stream.try_clone().unwrap())),
        );
        match ret {
            Ok(r) => {
                let body = r.body();
                let len = body.len() as u64;
                writer.write_all(&[0])?;
                writer.write_all(&len.to_be_bytes())?;
                writer.write_all(body)?;
                if let Some(json) = r.options_json() {
                    writer.write_all(json.to_string().as_bytes())?;
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
