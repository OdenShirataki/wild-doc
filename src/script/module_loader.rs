use deno_core::{
    error::AnyError, futures::future::FutureExt, ModuleLoader, ModuleSource, ModuleSpecifier,
    ModuleType,
};
use deno_runtime::deno_core;
use std::{pin::Pin, rc::Rc, str};

pub struct WdModuleLoader;

impl WdModuleLoader {
    pub fn new() -> Rc<Self> {
        Rc::new(Self)
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
        async move {
            let url = module_specifier.as_str();
            let module_type = if url.ends_with(".json") {
                ModuleType::Json
            } else {
                ModuleType::JavaScript
            };
            let body = reqwest::blocking::get(url)?.text()?;
            let module = ModuleSource {
                code: body.as_bytes().to_vec().into_boxed_slice(),
                module_type,
                module_url_specified: module_specifier.to_string(),
                module_url_found: module_specifier.to_string(),
            };
            Ok(module)
        }
        .boxed_local()
    }
}
