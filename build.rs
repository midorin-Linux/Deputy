use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=README.md");
    println!("cargo:rerun-if-changed=settings.example.yml");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // OUT_DIR = target/<profile>/build/<pkg>-<hash>/out
    let target_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("failed to locate target profile directory from OUT_DIR")
        .to_path_buf();

    let readme_src = manifest_dir.join("README.md");
    let settings_example_src = manifest_dir.join("settings.example.yml");

    if readme_src.exists() {
        fs::copy(&readme_src, target_dir.join("README.md"))
            .expect("failed to copy README.md to target directory");
    }

    // 生成するのは「雛形がまだ無いとき」だけ。既存の`settings.yml`には利用者が
    // トークン等を書き込んでいるため、ビルドのたびに例で上書きすると設定が消える。
    let settings_dst = target_dir.join("settings.yml");
    if settings_example_src.exists() && !settings_dst.exists() {
        fs::copy(&settings_example_src, &settings_dst)
            .expect("failed to copy settings.example.yml to settings.yml in target directory");
    }
}
