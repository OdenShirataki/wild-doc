use deno_core::{
    error::AnyError, futures::future::FutureExt, ModuleLoader, ModuleSource, ModuleSpecifier,
    ModuleType,
};
use deno_runtime::{
    deno_core,
    deno_fetch::{create_http_client, reqwest::Url},
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
        _is_main: bool,
    ) -> Result<ModuleSpecifier, AnyError> {
        if specifier.starts_with("wd://") {
            ModuleSpecifier::parse(specifier).map_err(|err| err.into())
        } else {
            let referrer = deno_runtime::deno_core::resolve_url_or_path(referrer).unwrap();
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
                        let client =
                            create_http_client("wild-doc".into(), None, vec![], None, None, None)?;
                        let resp = client.get(module_specifier.to_string()).send().await?;
                        if let Ok(resp) = resp.bytes().await {
                            let resp = resp.to_vec();
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
                            resp
                        } else {
                            vec![]
                        }
                    }
                } else {
                    b"".to_vec()
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

            let module_type = if module_specifier.to_string().ends_with(".json") {
                ModuleType::Json
            } else {
                ModuleType::JavaScript
            };

            let module = ModuleSource {
                code: code.into_boxed_slice(),
                module_type,
                module_url_specified: module_specifier.to_string(),
                module_url_found: module_specifier.to_string(),
            };
            Ok(module)
        }
        .boxed_local()
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
