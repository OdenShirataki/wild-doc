use std::path::PathBuf;

fn main() {
    // Build scripts must only write into OUT_DIR.
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    deno_runtime::snapshot::create_runtime_snapshot(
        out_dir.join("runtime.bin"),
        Default::default(),
    );
}
