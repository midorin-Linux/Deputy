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
    //
    // 生成先も変更追跡の対象にする。追跡ファイルは「削除された」場合も変更扱いに
    // なるため、利用者が設定をリセットしようと消しても次のビルドで再生成される。
    // （存在する間は書き換えないので、これが毎回の再実行を招くことはない。）
    let settings_dst = target_dir.join("settings.yml");
    println!("cargo:rerun-if-changed={}", settings_dst.display());
    if settings_example_src.exists() && !settings_dst.exists() {
        fs::copy(&settings_example_src, &settings_dst)
            .expect("failed to copy settings.example.yml to settings.yml in target directory");
    }
}
