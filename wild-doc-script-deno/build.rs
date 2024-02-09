use std::{
    fs::{self, File},
    io::Read,
};

fn main() {
    deno_runtime::snapshot::create_runtime_snapshot("runtime.bin.tmp".into(), Default::default());
    if let Ok(mut f) = File::open("runtime.bin") {
        let mut t=File::open("runtime.bin.tmp").unwrap();
        let mut fv = vec![];
        f.read_to_end(&mut fv).unwrap();

        let mut tv = vec![];
        t.read_to_end(&mut tv).unwrap();

        if fv != tv {
            fs::remove_file("runtime.bin").unwrap();
            fs::rename("runtime.bin.tmp", "runtime.bin").unwrap();
        }else{
            fs::remove_file("runtime.bin.tmp").unwrap();
        }
    } else {
        fs::rename("runtime.bin.tmp", "runtime.bin").unwrap();
    }
}
