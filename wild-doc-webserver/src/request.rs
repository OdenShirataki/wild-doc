use base64::{Engine as _, engine::general_purpose};
use bytes::Bytes;
use futures::stream::once;
use hyper::{
    Body, HeaderMap, Method, Request, Response, StatusCode, header::HeaderName, http::HeaderValue,
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

/// Returns true if `path` only consists of normal names, so that joining it to
/// a directory cannot point outside of that directory.
fn is_plain_relative_path(path: &str) -> bool {
    !path.contains(':')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

fn get_static_filename(document_root: &Path, hostname: &str, path: &str) -> Option<PathBuf> {
    if path.ends_with("/index.html") {
        return None;
    }
    let path = path.strip_prefix('/').unwrap_or(path);
    // the host name and the path come from the request, so they must not
    // point outside of the document root.
    if hostname.is_empty()
        || hostname.contains(['/', '\\'])
        || !is_plain_relative_path(hostname)
        || !(path.is_empty() || is_plain_relative_path(path))
    {
        return None;
    }
    let mut filename = document_root.to_path_buf();
    filename.push(hostname);
    filename.push("static");
    filename.push(path);
    if path.is_empty() || path.ends_with('/') {
        filename.push("index.html");
    }
    filename.is_file().then_some(filename)
}

#[cfg(test)]
mod tests {
    use super::get_static_filename;

    #[test]
    fn static_file_stays_in_document_root() {
        let root = std::env::temp_dir().join("wild-doc-webserver-static-test");
        let dir = root.join("localhost").join("static").join("css");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.css"), "a").unwrap();
        std::fs::write(
            root.join("localhost").join("static").join("index.html"),
            "i",
        )
        .unwrap();
        std::fs::write(root.join("secret.txt"), "s").unwrap();

        assert!(get_static_filename(&root, "localhost", "/css/a.css").is_some());
        assert!(get_static_filename(&root, "localhost", "/").is_some());
        assert!(get_static_filename(&root, "localhost", "/css/").is_none());

        for path in [
            "/../../secret.txt",
            "/css/../../../secret.txt",
            "/..\\..\\secret.txt",
            "/C:/Windows/win.ini",
            "//etc/passwd",
        ] {
            assert!(
                get_static_filename(&root, "localhost", path).is_none(),
                "{path}"
            );
        }
        for host in ["..", ".", "", "../localhost", "a/b", "a\\b", "C:"] {
            assert!(
                get_static_filename(&root, host, "/css/a.css").is_none(),
                "{host}"
            );
        }
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
        if let Some(host) = host.split(":").collect::<Vec<_>>().first() {
            let host = host.to_string();
            let uri_path = req.uri().path().to_owned();
            if let Some(static_file) = get_static_filename(&document_dir, &host, &uri_path) {
                if let Some(mime) = mime_infer::from_path(&static_file).first() {
                    let mut header_map = HeaderMap::new();
                    header_map.insert("Content-Type", mime.essence_str().parse().unwrap());
                    *response.headers_mut() = header_map;
                }
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
                let params_all = match *req.method() {
                    Method::GET => {
                        if let Ok(headers) = serde_json::to_string(&headers) {
                            if let Ok(headers) = serde_json::from_str(&headers) {
                                params_all.insert("headers".to_owned(), headers);
                            }
                        }
                        Some(params_all)
                    }
                    Method::POST => {
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
                                            if !params.is_empty() {
                                                params += "&";
                                            }
                                            let key = boundary.to_owned()
                                                + "-"
                                                + &files.len().to_string();
                                            params += &urlencoding::encode(name);
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
                                                if !params.is_empty() {
                                                    params += "&";
                                                }
                                                params += &urlencoding::encode(name);
                                                params += "=";
                                                params += &urlencoding::encode(v);
                                            }
                                        }
                                    }
                                }
                            }
                            if !params.is_empty() {
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
                                    if let Some(serde_json::Value::Object(headers)) =
                                        result_options.get("headers")
                                    {
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
