use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use futures::stream::once;
use hyper::{
    header::HeaderName, http::HeaderValue, Body, HeaderMap, Method, Request, Response, StatusCode,
};
use multer::Multipart;
use serde_json::Value;
use serde_querystring::BracketsQS;
use std::{
    collections::HashMap,
    convert::Infallible,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use wild_doc_client_lib::WildDocClient;

fn get_static_filename(document_root: &Path, hostname: &str, path: &str) -> Option<PathBuf> {
    if path.ends_with("/index.html") != true {
        let mut filename = document_root.to_path_buf();
        filename.push(hostname);
        filename.push("static");
        let mut path = path.to_owned();
        path.remove(0);
        filename.push(&path);
        if path.ends_with("/") {
            filename.push("index.html");
        }
        if filename.exists() {
            Some(filename)
        } else {
            None
        }
    } else {
        None
    }
}

#[derive(Serialize)]
struct UploadFile {
    file_name: String,
    content_type: String,
    len: usize,
    data: String, //(base64)
}
struct UploadFileWrapper {
    key: String,
    file: UploadFile,
}
fn parse_brakets_qs(qs: BracketsQS) -> Value {
    let mut json = serde_json::json!({});
    if let Some(json) = json.as_object_mut() {
        for key in qs.keys() {
            if let Ok(str_key) = std::str::from_utf8(key) {
                if let Some(Some(value)) = qs.value(key) {
                    if let Ok(value) = std::str::from_utf8(&value) {
                        json.insert(str_key.to_owned(), value.into());
                    }
                } else {
                    if let Some(values) = qs.sub_values(key) {
                        json.insert(str_key.to_owned(), parse_brakets_qs(values));
                    }
                }
            }
        }
    }
    json
}
fn parse_qs(query_string: &[u8]) -> Value {
    parse_brakets_qs(BracketsQS::parse(query_string))
}
pub(super) async fn request(
    wd_host: String,
    wd_port: String,
    document_dir: String,
    req: Request<Body>,
) -> Result<Response<Body>, hyper::Error> {
    let document_dir = std::path::PathBuf::from(document_dir);
    let mut response = Response::new(Body::empty());

    let headers: HashMap<_, _> = req
        .headers()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_owned()))
        .collect();
    if let Some(host) = headers.get("host") {
        if let Some(host) = host.split(":").collect::<Vec<_>>().get(0) {
            let host = host.to_string();
            let uri_path = req.uri().path().to_owned();
            if let Some(static_file) = get_static_filename(&document_dir, &host, &uri_path) {
                let mut f = File::open(static_file).unwrap();
                let mut buf = Vec::new();
                f.read_to_end(&mut buf).unwrap();
                *response.body_mut() = Body::from(buf);
            } else {
                let mut wdc = WildDocClient::new(&wd_host, &wd_port, &document_dir, &host);

                let mut params_all: HashMap<String, serde_json::Value> = HashMap::new();

                params_all.insert(
                    "uri".to_owned(),
                    serde_json::Value::String(uri_path.clone()),
                );
                params_all.insert("path".to_owned(), serde_json::Value::String(uri_path));

                if let Some(query) = req.uri().query() {
                    params_all.insert("query".to_owned(), query.into());
                    params_all.insert("get".to_owned(), parse_qs(query.as_bytes()));
                }

                let mut files = vec![];
                let params_all = match req.method() {
                    &Method::GET => {
                        if let Ok(headers) = serde_json::to_string(&headers) {
                            if let Ok(headers) = serde_json::from_str(&headers) {
                                params_all.insert("headers".to_owned(), headers);
                            }
                        }
                        Some(params_all)
                    }
                    &Method::POST => {
                        let content_type = headers.get("content-type").unwrap();
                        let body = hyper::body::to_bytes(req.into_body()).await?;

                        if content_type == "application/x-www-form-urlencoded" {
                            params_all.insert("post".to_owned(), parse_qs(body.as_ref()));
                        } else if content_type.starts_with("multipart/form-data;") {
                            let boundary: Vec<_> = content_type.split("boundary=").collect();
                            let boundary = boundary[1];
                            let mut multipart = Multipart::new(
                                once(async move { Result::<Bytes, Infallible>::Ok(body) }),
                                boundary,
                            );
                            let mut params = "".to_owned();
                            while let Some(mut field) = multipart.next_field().await.unwrap() {
                                while let Some(chunk) = field.chunk().await.unwrap() {
                                    if let Some(name) = field.name() {
                                        if let (Some(file_name), Some(content_type)) =
                                            (field.file_name(), field.content_type())
                                        {
                                            if params.len() > 0 {
                                                params += "&";
                                            }
                                            let key = boundary.to_owned()
                                                + "-"
                                                + &files.len().to_string();
                                            params += &urlencoding::encode(name).into_owned();
                                            params += "=";
                                            params += &key;
                                            files.push(UploadFileWrapper {
                                                key,
                                                file: UploadFile {
                                                    file_name: file_name.to_owned(),
                                                    content_type: content_type.to_string(),
                                                    len: chunk.len(),
                                                    data: general_purpose::STANDARD_NO_PAD
                                                        .encode(chunk),
                                                },
                                            });
                                        } else {
                                            if let Ok(v) = std::str::from_utf8(&chunk) {
                                                if params.len() > 0 {
                                                    params += "&";
                                                }
                                                params += &urlencoding::encode(name).into_owned();
                                                params += "=";
                                                params += &urlencoding::encode(v).into_owned();
                                            }
                                        }
                                    }
                                }
                            }
                            if params.len() > 0 {
                                params_all.insert("post".to_owned(), parse_qs(params.as_bytes()));
                            }
                        }

                        if let Ok(headers) = serde_json::to_string(&headers) {
                            if let Ok(headers) = serde_json::from_str(&headers) {
                                params_all.insert("headers".to_owned(), headers);
                            }
                        }
                        Some(params_all)
                    }
                    _ => None,
                };
                if let Some(params_all) = params_all {
                    if let Ok(mut json) = serde_json::to_string(&params_all) {
                        for f in files {
                            if let Ok(file) = serde_json::to_string(&f.file) {
                                let key = "\"".to_string() + &f.key + "\"";
                                json = json.replace(&key, &file);
                            }
                        }
                        let mut filename = document_dir.clone();
                        filename.push(host);
                        filename.push("request.xml");
                        let mut f = File::open(filename).unwrap();
                        let mut xml = String::new();
                        f.read_to_string(&mut xml).unwrap();
                        match wdc.exec(&xml, &json) {
                            Ok(r) => {
                                let result_options =
                                    serde_json::from_str::<HashMap<String, serde_json::Value>>(
                                        r.options_json(),
                                    );

                                let mut need_response_body = true;
                                if let Ok(result_options) = result_options {
                                    let mut response_status = StatusCode::FOUND;
                                    if let Some(status) = result_options.get("status") {
                                        let status = status.to_string();
                                        if status == "404" {
                                            response_status = StatusCode::NOT_FOUND;
                                        }
                                    }
                                    let mut response_headers = HeaderMap::new();
                                    if let Some(headers) = result_options.get("headers") {
                                        if let serde_json::Value::Object(headers) = headers {
                                            for (k, v) in headers {
                                                if let Some(v) = v.as_str() {
                                                    if let (Ok(k), Ok(v)) = (
                                                        HeaderName::from_bytes(k.as_bytes()),
                                                        HeaderValue::from_str(v),
                                                    ) {
                                                        response_headers.insert(k, v);
                                                    }
                                                } else {
                                                    if let (Ok(k), Ok(v)) = (
                                                        HeaderName::from_bytes(k.as_bytes()),
                                                        HeaderValue::from_str(&v.to_string()),
                                                    ) {
                                                        response_headers.insert(k, v);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if response_headers.contains_key("location") {
                                        response_status = StatusCode::SEE_OTHER;
                                        need_response_body = false;
                                    }
                                    *response.headers_mut() = response_headers;
                                    if response_status != StatusCode::FOUND {
                                        *response.status_mut() = response_status;
                                    }
                                }
                                if need_response_body {
                                    *response.body_mut() = Body::from(r.body().to_vec());
                                }
                            }
                            Err(e) => {
                                eprintln!("{:?}", e);
                                *response.body_mut() = Body::from(e.to_string());
                            }
                        }
                    }
                } else {
                    *response.status_mut() = StatusCode::NOT_FOUND;
                }
            }
        }
    }
    Ok(response)
}
