use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("keyfix/lib.rs");
    let so = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("libvortex_keyfix.so");
    println!("cargo:rerun-if-changed={}", src.display());
    let rustc = std::env::var("RUSTC").expect("RUSTC");
    let status = Command::new(rustc)
        .arg("--edition=2024")
        .arg("--crate-type=cdylib")
        .arg("--crate-name=vortex_keyfix")
        .arg("-C")
        .arg("panic=abort")
        .arg("-o")
        .arg(&so)
        .arg(&src)
        .status()
        .expect("run rustc for keyfix");
    if !status.success() {
        panic!("failed to build libvortex_keyfix.so");
    }
    println!("cargo:rustc-env=VORTEX_KEYFIX={}", so.display());
    tauri_build::build()
}
