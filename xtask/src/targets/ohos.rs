//! Builds for `aarch64-unknown-linux-ohos` with the HarmonyOS SDK.
//!
//! The SDK ships no OpenSSL, so the manifest builds it from source here as well. Its toolchain has
//! no target-prefixed wrapper either (unlike the NDK), so the clang that compiles the C has to be
//! told the target and the sysroot itself, and it has to be named in `TARGET_CC` because
//! `libquickjs-ng-sys` calls `cc` with a bare `clang`. Nothing is linked — the build is a static
//! library on its own and no dependency declares a `cdylib` any more — so no linker is set up
//! here.

use std::env;
use std::path::{Path, PathBuf};

use crate::util;

const TARGET: &str = "aarch64-unknown-linux-ohos";
/// The triple clang itself is told; this SDK has no wrapper that would carry it.
const CLANG_TARGET: &str = "aarch64-linux-ohos";

pub fn build(sdk_root: Option<&Path>, release: bool) -> Result<(), String> {
    let sdk = sdk_root
        .map(Path::to_path_buf)
        .or_else(|| env::var_os("OHOS_SDK_ROOT").map(PathBuf::from))
        .or_else(|| env::var_os("HARMONYOS_SDK_ROOT").map(PathBuf::from))
        .or_else(|| env::var_os("HARMONYOS_NDK_ROOT").map(PathBuf::from))
        .ok_or("no HarmonyOS SDK was found; set `OHOS_SDK_ROOT` or pass `--sdk-root`")?;

    // The toolchain is wherever the SDK keeps `llvm-ar`; its layout has changed between releases.
    let llvm_ar = util::find_file(&sdk, "llvm-ar", 8)
        .ok_or_else(|| format!("no `llvm-ar` was found under {}", sdk.display()))?;
    let bin = llvm_ar
        .parent()
        .expect("a file has a directory")
        .to_path_buf();
    let clang = util::tool_in(&bin, "clang")
        .or_else(|| util::tool_like(&bin, "clang-"))
        .ok_or_else(|| format!("no `clang` in {}", bin.display()))?;
    let ranlib = util::tool_in(&bin, "llvm-ranlib")
        .or_else(|| util::tool_in(&bin, "ranlib"))
        .ok_or_else(|| format!("no `llvm-ranlib` in {}", bin.display()))?;

    // The sysroot is next to the directory that holds the toolchain (`<sdk>/native/sysroot` for
    // `<sdk>/native/llvm/bin`), and some packages call it `sysroot_lite` instead. Only the one
    // with the C headers is usable.
    let sysroot = ["sysroot", "sysroot_lite"]
        .into_iter()
        .map(|name| bin.join(name))
        .find(|candidate| candidate.join("usr/include/stdio.h").is_file())
        .or_else(|| {
            bin.parent().and_then(Path::parent).and_then(|native| {
                ["sysroot", "sysroot_lite"]
                    .into_iter()
                    .map(|name| native.join(name))
                    .find(|candidate| candidate.join("usr/include/stdio.h").is_file())
            })
        })
        .or_else(|| {
            ["sysroot", "sysroot_lite"]
                .into_iter()
                .filter_map(|name| util::find_dir(&sdk, name, 8))
                .find(|candidate| candidate.join("usr/include/stdio.h").is_file())
        })
        .ok_or_else(|| format!("no sysroot was found under {}", sdk.display()))?;

    println!("sdk:       {}", sdk.display());
    println!("toolchain: {}", bin.display());
    println!("sysroot:   {}", sysroot.display());

    let toolchain = util::Clang {
        dir: &bin,
        clang: &clang,
        ar: &llvm_ar,
        ranlib: &ranlib,
        sysroot: &sysroot,
    };

    util::cargo_build(TARGET, release, &toolchain.env(TARGET, CLANG_TARGET))
}
