use futures_util::future::join;
use hyper::{
    server::conn::{AddrIncoming, Http},
    service::{make_service_fn, service_fn},
    Server,
};
use once_cell::sync::Lazy;
use rustls::server::ResolvesServerCertUsingSni;
use std::{
    collections::HashMap,
    io::Read,
    net::SocketAddr,
    sync::Arc,
    {io, sync},
};
use tokio::net::TcpListener;

mod request;
mod tls;

#[macro_use]
extern crate serde_derive;

#[derive(Deserialize)]
struct Config {
    wilddoc: Option<ConfigWildDoc>,
}
#[derive(Deserialize)]
struct ConfigWildDoc {
    server_addr: Option<String>,
    server_port: Option<String>,
    document_dir: Option<String>,
}

static SETTING: Lazy<std::sync::Mutex<HashMap<String, String>>> = Lazy::new(|| {
    let mut m = HashMap::new();
    if let Ok(mut f) = std::fs::File::open("./wild-doc-webserver.toml") {
        let mut toml = String::new();
        if let Ok(_) = f.read_to_string(&mut toml) {
            let config: Result<Config, toml::de::Error> = toml::from_str(&toml);
            if let Ok(config) = config {
                if let Some(config) = config.wilddoc {
                    m.insert(
                        "server_addr".to_string(),
                        if let Some(server_addr) = config.server_addr {
                            server_addr
                        } else {
                            "localhost".to_string()
                        },
                    );
                    m.insert(
                        "server_port".to_string(),
                        if let Some(server_port) = config.server_port {
                            server_port
                        } else {
                            "51818".to_string()
                        },
                    );
                    m.insert(
                        "document_dir".to_string(),
                        if let Some(document_dir) = config.document_dir {
                            document_dir
                        } else {
                            "document".to_string()
                        },
                    );
                }
            }
        }
    }
    std::sync::Mutex::new(m)
});

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let http_server = {
        let addr_http: SocketAddr = ([127, 0, 0, 1], 80).into();
        async move {
            let listener = TcpListener::bind(addr_http).await.unwrap();
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::task::spawn(async move {
                    if let Err(err) = Http::new()
                        .serve_connection(
                            stream,
                            service_fn(move |req| {
                                let addr = SETTING
                                    .lock()
                                    .unwrap()
                                    .get("server_addr")
                                    .unwrap()
                                    .to_owned();
                                let port = SETTING
                                    .lock()
                                    .unwrap()
                                    .get("server_port")
                                    .unwrap()
                                    .to_owned();
                                let document_dir = SETTING
                                    .lock()
                                    .unwrap()
                                    .get("document_dir")
                                    .unwrap()
                                    .to_owned();
                                request::request(addr, port, document_dir, req)
                            }),
                        )
                        .await
                    {
                        println!("Error serving connection: {:?}", err);
                    }
                });
            }
        }
    };

    let https_server = {
        // Serve an echo service over HTTPS, with proper error handling.
        let addr_https: SocketAddr = ([127, 0, 0, 1], 443).into();
        async move {
            let cfg = rustls::ServerConfig::builder()
                .with_safe_defaults()
                .with_no_client_auth();
            let cfg = if true {
                cfg.with_single_cert(
                    tls::load_certs("certificates/localhost/fullchain.pem").unwrap(),
                    tls::load_private_key("certificates/localhost/privkey.pem").unwrap(),
                )
                .map_err(|e| tls::error(format!("{}", e)))
                .unwrap()
            } else {
                let mut resolver = ResolvesServerCertUsingSni::new();
                tls::add_certificate_to_resolver("localhost", "localhost", &mut resolver);
                cfg.with_cert_resolver(Arc::new(resolver))
            };
            // Configure ALPN to accept HTTP/2, HTTP/1.1 in that order.
            //cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

            // Create a TCP listener via tokio.
            Server::builder(tls::TlsAcceptor::new(
                sync::Arc::new(cfg),
                AddrIncoming::bind(&addr_https)?,
            ))
            .serve(make_service_fn(|_| async {
                Ok::<_, io::Error>(service_fn(|req| {
                    let addr = SETTING
                        .lock()
                        .unwrap()
                        .get("server_addr")
                        .unwrap()
                        .to_owned();
                    let port = SETTING
                        .lock()
                        .unwrap()
                        .get("server_port")
                        .unwrap()
                        .to_owned();
                    let document_dir = SETTING
                        .lock()
                        .unwrap()
                        .get("document_dir")
                        .unwrap()
                        .to_owned();
                    request::request(addr, port, document_dir, req)
                }))
            }))
            .await
        }
    };
    let _ret = join(https_server, http_server).await;
    Ok(())
}
