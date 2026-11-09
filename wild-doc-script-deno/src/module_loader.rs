use std::path::PathBuf;

use deno_error::JsErrorBox;
use deno_runtime::deno_core::{
    ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader, ModuleResolveResponse,
    ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, ResolutionKind,
    futures::FutureExt, resolve_import, url::Url,
};
use ring::digest::{Context, SHA256};

pub struct WdModuleLoader {
    module_cache_dir: PathBuf,
}

impl WdModuleLoader {
    pub fn new(module_cache_dir: PathBuf) -> Self {
        Self { module_cache_dir }
    }
}

impl ModuleLoader for WdModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> ModuleResolveResponse {
        if specifier.starts_with("wd://") {
            ModuleSpecifier::parse(specifier).map_err(JsErrorBox::from_err)
        } else {
            resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)
        }
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let module_specifier = module_specifier.clone();
        let module_cache_dir = self.module_cache_dir.clone();
        ModuleLoadResponse::Async(
            async move {
                let code = if module_specifier.scheme().starts_with("http") {
                    load_remote(&module_specifier, module_cache_dir).await?
                } else {
                    let path = module_specifier.to_file_path().map_err(|_| {
                        JsErrorBox::generic(format!(
                            "Provided module specifier \"{module_specifier}\" is not a file URL."
                        ))
                    })?;
                    std::fs::read(path).map_err(JsErrorBox::from_err)?
                };
                let code =
                    String::from_utf8(code).map_err(|e| JsErrorBox::generic(e.to_string()))?;

                let specifier = module_specifier.as_str();
                let code = if specifier.ends_with(".ts") {
                    transpile(&module_specifier, code)?
                } else {
                    code
                };
                Ok(ModuleSource::new(
                    if specifier.ends_with(".json") {
                        ModuleType::Json
                    } else {
                        ModuleType::JavaScript
                    },
                    ModuleSourceCode::String(code.into()),
                    &module_specifier,
                    None,
                ))
            }
            .boxed_local(),
        )
    }
}

/// Loads a remote module, using the module cache directory.
async fn load_remote(url: &Url, mut cache_path: PathBuf) -> Result<Vec<u8>, JsErrorBox> {
    let cache_filename = url_to_filename(url)
        .ok_or_else(|| JsErrorBox::generic(format!("Can not cache the module \"{url}\".")))?;
    cache_path.push(cache_filename);
    if let Ok(code) = std::fs::read(&cache_path) {
        return Ok(code);
    }

    // reqwest follows redirects.
    let response = reqwest::get(url.as_str())
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    let code = response
        .bytes()
        .await
        .map_err(|e| JsErrorBox::generic(e.to_string()))?
        .to_vec();
    if !code.is_empty()
        && let Some(dir) = cache_path.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = std::fs::write(&cache_path, &code);
    }
    Ok(code)
}

fn transpile(specifier: &ModuleSpecifier, code: String) -> Result<String, JsErrorBox> {
    let parsed = deno_ast::parse_module(deno_ast::ParseParams {
        specifier: specifier.clone(),
        text: code.into(),
        media_type: deno_ast::MediaType::TypeScript,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    })
    .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    Ok(parsed
        .transpile(
            &Default::default(),
            &Default::default(),
            &Default::default(),
        )
        .map_err(|e| JsErrorBox::generic(e.to_string()))?
        .into_source()
        .text)
}

fn url_to_filename(url: &Url) -> Option<PathBuf> {
    let mut cache_filename = base_url_to_filename(url)?;

    let mut rest_str = url.path().to_string();
    if let Some(query) = url.query() {
        rest_str.push('?');
        rest_str.push_str(query);
    }
    cache_filename.push(checksum_gen(&[rest_str.as_bytes()]));
    Some(cache_filename)
}

fn base_url_to_filename(url: &Url) -> Option<PathBuf> {
    let mut out = PathBuf::new();

    let scheme = url.scheme();
    out.push(scheme);

    match scheme {
        "http" | "https" => {
            let host = url.host_str()?;
            let host_port = match url.port() {
                Some(port) => format!("{host}_PORT{port}"),
                None => host.to_string(),
            };
            out.push(host_port);
        }
        "data" | "blob" => (),
        scheme => {
            eprintln!("Don't know how to create cache name for scheme: {scheme}");
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
    ctx.finish()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
