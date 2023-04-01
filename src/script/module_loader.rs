use deno_ast;
use deno_core::{
    error::AnyError, futures::future::FutureExt, ModuleLoader, ModuleSource, ModuleSpecifier,
    ModuleType,
};
use deno_runtime::{
    deno_core::{
        self,
        error::{custom_error, generic_error},
        ResolutionKind,
    },
    deno_fetch::{
        create_http_client,
        reqwest::{self, header::LOCATION, Response, Url},
    },
};
use log::error;
use ring::digest::{Context, SHA256};
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::PathBuf,
    pin::Pin,
    rc::Rc,
    str,
};

pub struct WdModuleLoader {
    module_cache_dir: PathBuf,
}
impl WdModuleLoader {
    pub fn new(module_cache_dir: PathBuf) -> Rc<Self> {
        Rc::new(Self { module_cache_dir })
    }
}

impl ModuleLoader for WdModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, AnyError> {
        if specifier.starts_with("wd://") {
            ModuleSpecifier::parse(specifier).map_err(|err| err.into())
        } else {
            let referrer = deno_runtime::deno_core::resolve_url(referrer)?;
            deno_runtime::deno_core::resolve_import(specifier, referrer.as_str())
                .map_err(|err| err.into())
        }
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<ModuleSpecifier>,
        _is_dynamic: bool,
    ) -> Pin<Box<deno_core::ModuleSourceFuture>> {
        let module_specifier = module_specifier.clone();
        let mut module_cache_path = self.module_cache_dir.clone();
        async move {
            let code = if module_specifier.scheme().starts_with("http") {
                if let Some(cache_filename) = url_to_filename(&module_specifier) {
                    module_cache_path.push(cache_filename);
                    if module_cache_path.exists() {
                        let mut buf: Vec<u8> = vec![];
                        if let Ok(mut file) = File::open(module_cache_path) {
                            let _ = file.read_to_end(&mut buf);
                        }
                        buf
                    } else {
                        let url = module_specifier.to_string();
                        let client =
                            create_http_client("wild-doc".into(), None, vec![], None, None, None)?;
                        let resp = get_redirected_response(&client, url).await?;
                        match resp.bytes().await {
                            Ok(resp) => {
                                let resp = resp.to_vec();
                                if resp.len() > 0 {
                                    if let Some(dir) = module_cache_path.parent() {
                                        if let Ok(()) = std::fs::create_dir_all(dir) {
                                            if let Ok(file) = OpenOptions::new()
                                                .create(true)
                                                .write(true)
                                                .truncate(true)
                                                .open(module_cache_path)
                                            {
                                                let _ = BufWriter::new(file).write_all(&resp);
                                            }
                                        }
                                    }
                                }
                                resp
                            }
                            Err(e) => {
                                println!("{:?}", e);
                                vec![]
                            }
                        }
                    }
                } else {
                    vec![]
                }
            } else {
                let path = module_specifier.to_file_path().map_err(|_| {
                    deno_core::error::generic_error(format!(
                        "Provided module specifier \"{}\" is not a file URL.",
                        module_specifier
                    ))
                })?;
                std::fs::read(path)?
            };

            let string_specifier = module_specifier.to_string();
            let module_type = if string_specifier.ends_with(".json") {
                ModuleType::Json
            } else {
                ModuleType::JavaScript
            };
            let code = if string_specifier.ends_with(".ts") {
                let parse_soure = deno_ast::parse_module(deno_ast::ParseParams {
                    specifier: string_specifier,
                    text_info: deno_ast::SourceTextInfo::new(
                        std::str::from_utf8(&code).unwrap().into(),
                    ),
                    media_type: deno_ast::MediaType::TypeScript,
                    capture_tokens: true,
                    scope_analysis: true,
                    maybe_syntax: None,
                })
                .unwrap();
                let transpiled_source = parse_soure.transpile(&Default::default()).unwrap();
                transpiled_source.text.as_bytes().to_vec()
            } else {
                code
            };
            let module = ModuleSource {
                code: code.into(),
                module_type,
                module_url_specified: module_specifier.to_string(),
                module_url_found: module_specifier.to_string(),
            };
            Ok(module)
        }
        .boxed_local()
    }
}

fn resolve_url_from_location(base_url: &Url, location: &str) -> Url {
    if location.starts_with("http://") || location.starts_with("https://") {
        // absolute uri
        Url::parse(location).expect("provided redirect url should be a valid url")
    } else if location.starts_with("//") {
        // "//" authority path-abempty
        Url::parse(&format!("{}:{}", base_url.scheme(), location))
            .expect("provided redirect url should be a valid url")
    } else if location.starts_with('/') {
        // path-absolute
        base_url
            .join(location)
            .expect("provided redirect url should be a valid url")
    } else {
        // assuming path-noscheme | path-empty
        let base_url_path_str = base_url.path().to_owned();
        // Pop last part or url (after last slash)
        let segs: Vec<&str> = base_url_path_str.rsplitn(2, '/').collect();
        let new_path = format!("{}/{}", segs.last().unwrap_or(&""), location);
        base_url
            .join(&new_path)
            .expect("provided redirect url should be a valid url")
    }
}
fn resolve_redirect_from_response(request_url: &Url, response: &Response) -> Result<Url, AnyError> {
    debug_assert!(response.status().is_redirection());
    if let Some(location) = response.headers().get(LOCATION) {
        let location_string = location.to_str().unwrap();
        log::debug!("Redirecting to {:?}...", &location_string);
        let new_url = resolve_url_from_location(request_url, location_string);
        Ok(new_url)
    } else {
        Err(generic_error(format!(
            "Redirection from '{}' did not provide location header",
            request_url
        )))
    }
}
async fn get_redirected_response<U: reqwest::IntoUrl>(
    client: &reqwest::Client,
    url: U,
) -> Result<Response, AnyError> {
    let mut url = url.into_url()?;
    let mut response = client.get(url.clone()).send().await?;
    let status = response.status();
    if status.is_redirection() {
        for _ in 0..5 {
            let new_url = resolve_redirect_from_response(&url, &response)?;
            let new_response = client.get(new_url.clone()).send().await?;
            let status = new_response.status();
            if status.is_redirection() {
                response = new_response;
                url = new_url;
            } else {
                return Ok(new_response);
            }
        }
        Err(custom_error("Http", "Too many redirects."))
    } else {
        Ok(response)
    }
}

fn url_to_filename(url: &Url) -> Option<PathBuf> {
    let mut cache_filename = base_url_to_filename(url)?;

    let mut rest_str = url.path().to_string();
    if let Some(query) = url.query() {
        rest_str.push('?');
        rest_str.push_str(query);
    }
    let hashed_filename = checksum_gen(&[rest_str.as_bytes()]);
    cache_filename.push(hashed_filename);
    Some(cache_filename)
}

fn base_url_to_filename(url: &Url) -> Option<PathBuf> {
    let mut out = PathBuf::new();

    let scheme = url.scheme();
    out.push(scheme);

    match scheme {
        "http" | "https" => {
            let host = url.host_str().unwrap();
            let host_port = match url.port() {
                Some(port) => format!("{}_PORT{}", host, port),
                None => host.to_string(),
            };
            out.push(host_port);
        }
        "data" | "blob" => (),
        scheme => {
            error!("Don't know how to create cache name for scheme: {}", scheme);
            return None;
        }
    };

    Some(out)
}
pub fn checksum_gen(v: &[impl AsRef<[u8]>]) -> String {
    let mut ctx = Context::new(&SHA256);
    for src in v {
        ctx.update(src.as_ref());
    }
    let digest = ctx.finish();
    let out: Vec<String> = digest
        .as_ref()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect();
    out.join("")
}
