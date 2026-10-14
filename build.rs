use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=src/platform/macos.m");
    println!("cargo:rerun-if-changed=src/platform/audio.c");
    println!("cargo:rerun-if-changed=src/platform/font.c");
    println!("cargo:rerun-if-changed=src/platform/metal.m");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    // Keep the IR segment executors' instruction layout independent of Win32
    // handler sizes. A page boundary also preserves its cache-line offsets.
    println!("cargo:rustc-link-arg=-Wl,-sectalign,__TEXT,__wire_ir,0x1000");
    println!("cargo:rustc-link-arg=-Wl,-sectalign,__TEXT,__wire_irchk,0x1000");
    for section in [
        "__wire_ir32",
        "__wire_mix32",
        "__wire_simd32",
        "__wire_raw32",
        "__wire_0f32",
    ] {
        println!("cargo:rustc-link-arg=-Wl,-sectalign,__TEXT,{section},0x1000");
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
    assert!(
        Command::new("clang")
            .args([
                "-mmacosx-version-min=11.0",
                "-Wall",
                "-Wextra",
                "-c",
                "src/platform/audio.c",
                "-o"
            ])
            .arg(out.join("audio.o"))
            .status()
            .unwrap()
            .success(),
        "AudioQueue bridge compile failed"
    );
    assert!(
        Command::new("clang")
            .args([
                "-fobjc-arc",
                "-mmacosx-version-min=11.0",
                "-Wall",
                "-Wextra",
                "-c",
                "src/platform/metal.m",
                "-o"
            ])
            .arg(out.join("metal.o"))
            .status()
            .unwrap()
            .success(),
        "Metal bridge compile failed"
    );
    assert!(
        Command::new("clang")
            .args([
                "-mmacosx-version-min=11.0",
                "-Wall",
                "-Wextra",
                "-c",
                "src/platform/font.c",
                "-o"
            ])
            .arg(out.join("font.o"))
            .status()
            .unwrap()
            .success(),
        "CoreText bridge compile failed"
    );
    assert!(Command::new("ar")
        .arg("crs")
        .arg(out.join("libwire_ui.a"))
        .arg(out.join("macos.o"))
        .arg(out.join("audio.o"))
        .arg(out.join("metal.o"))
        .arg(out.join("font.o"))
        .status()
        .unwrap()
        .success());
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=wire_ui");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=QuartzCore");
    println!("cargo:rustc-link-lib=framework=AudioToolbox");
    println!("cargo:rustc-link-lib=framework=Metal");
    println!("cargo:rustc-link-lib=framework=CoreText");
}
