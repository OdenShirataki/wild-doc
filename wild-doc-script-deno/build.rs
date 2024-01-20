fn main() {
    deno_runtime::snapshot::create_runtime_snapshot("runtime.bin".into(), Default::default());
}
