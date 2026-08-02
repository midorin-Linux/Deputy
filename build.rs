use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=README.md");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // OUT_DIR = target/<profile>/build/<pkg>-<hash>/out
    let target_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("failed to locate target profile directory from OUT_DIR")
        .to_path_buf();

    let readme_src = manifest_dir.join("README.md");

    if readme_src.exists() {
        fs::copy(&readme_src, target_dir.join("README.md"))
            .expect("failed to copy README.md to target directory");
    }
}