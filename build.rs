use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=src/platform/macos.m");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let status = Command::new("clang")
        .args([
            "-fobjc-arc",
            "-mmacosx-version-min=11.0",
            "-Wall",
            "-Wextra",
            "-c",
            "src/platform/macos.m",
            "-o",
        ])
        .arg(out.join("macos.o"))
        .status()
        .expect("system clang");
    assert!(status.success(), "AppKit bridge compile failed");
    assert!(Command::new("ar")
        .arg("crs")
        .arg(out.join("libwire_ui.a"))
        .arg(out.join("macos.o"))
        .status()
        .unwrap()
        .success());
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=wire_ui");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
}
