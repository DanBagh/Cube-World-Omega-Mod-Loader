//! Builds the in-game loader, so `loader::embedded` can carry it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

/// A loader DLL to embed instead of building one.
const PREBUILT: &str = "CWOML_LOADER_DLL";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../loader/src");
    println!("cargo:rerun-if-changed=../loader/Cargo.toml");
    println!("cargo:rerun-if-env-changed={PREBUILT}");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let embedded = out.join("loader.dll");
    // built for Windows elsewhere, for a build on another system
    if let Some(dll) = env::var_os(PREBUILT) {
        println!("cargo:rerun-if-changed={}", Path::new(&dll).display());
        fs::copy(&dll, &embedded).unwrap_or_else(|e| panic!("{PREBUILT}: {e}"));
        return;
    }
    // the game is a Windows program, so elsewhere there is nothing to embed
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        fs::write(&embedded, []).expect("write the empty loader");
        return;
    }
    // a build folder of its own: this cargo would wait for ever on the one that runs it.
    // OUT_DIR is <target>/<profile>/build/<package>/out
    let target = out
        .ancestors()
        .nth(4)
        .map_or_else(|| out.clone(), Path::to_path_buf)
        .join("loader");
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut build = Command::new(cargo);
    // under `cargo clippy` these would have the loader linted in place of built
    build
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CLIPPY_ARGS")
        .args(["build", "--release", "--package", "cwoml-loader"])
        .arg("--target-dir")
        .arg(&target);
    if let Ok(triple) = env::var("TARGET") {
        build.args(["--target", &triple]);
    }
    let status = build.status().expect("run cargo for the loader");
    assert!(status.success(), "the loader did not build");
    let triple = env::var("TARGET").unwrap_or_default();
    let dll = target.join(triple).join("release/cwoml_loader.dll");
    fs::copy(&dll, &embedded).unwrap_or_else(|e| panic!("{}: {e}", dll.display()));
}
