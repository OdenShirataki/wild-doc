use std::{collections::HashSet, fmt::Write, path::PathBuf};

use deno_runtime::{
    ops::bootstrap::SnapshotOptions,
    snapshot::{LazyExtensionFileKind, create_runtime_snapshot},
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Build scripts must only write into OUT_DIR.
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let output = create_runtime_snapshot(
        out_dir.join("runtime.bin"),
        SnapshotOptions::default(),
        vec![],
    );

    // Lazily loaded sources that are not in the snapshot have to be passed to
    // the worker at runtime.
    let consumed: HashSet<_> = output.consumed_lazy_specifiers.iter().collect();
    let mut js = String::new();
    let mut esm = String::new();
    for file in &output.lazy_extension_files {
        if consumed.contains(&file.specifier) {
            continue;
        }
        let list = match file.kind {
            LazyExtensionFileKind::Js => &mut js,
            LazyExtensionFileKind::Esm => &mut esm,
        };
        writeln!(
            list,
            "    ({:?}, include_str!({:?})),",
            file.specifier,
            file.path.to_string_lossy()
        )
        .unwrap();
        println!("cargo:rerun-if-changed={}", file.path.display());
    }
    std::fs::write(
        out_dir.join("residual_sources.rs"),
        format!(
            "pub static RESIDUAL_LAZY_JS_SOURCES: &[(&str, &str)] = &[\n{js}];\n\
             pub static RESIDUAL_LAZY_ESM_SOURCES: &[(&str, &str)] = &[\n{esm}];\n"
        ),
    )
    .unwrap();
}
