//! Builds for `aarch64-linux-android` with the Android NDK.
//!
//! The NDK ships no OpenSSL (no headers, no import libraries), which is why the manifest builds it
//! from source for this target. Its `clang` wrappers carry the target and the API level in their
//! names, but `libquickjs-ng-sys` calls `cc` with a bare `clang` and ignores `CC_…`, so the
//! compiler is named in `TARGET_CC` and the flags are repeated in `CFLAGS_…` and
//! `BINDGEN_EXTRA_CLANG_ARGS_…` as well. Nothing is linked, so no linker is set up here.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::util;

const TARGET: &str = "aarch64-linux-android";
const CLANG: &str = "aarch64-linux-android24-clang";
/// The triple clang itself is told, which carries the API level, unlike the cargo one.
const CLANG_TARGET: &str = "aarch64-linux-android24";

pub fn build(sdk_root: Option<&Path>, release: bool) -> Result<(), String> {
    let ndk = sdk_root
        .map(Path::to_path_buf)
        .or_else(|| env::var_os("ANDROID_NDK_HOME").map(PathBuf::from))
        .or_else(|| env::var_os("ANDROID_NDK_ROOT").map(PathBuf::from))
        .or_else(|| env::var_os("ANDROID_NDK_LATEST_HOME").map(PathBuf::from))
        .or_else(ndk_from_sdk)
        .ok_or("no Android NDK was found; set `ANDROID_NDK_HOME` or pass `--sdk-root`")?;

    let prebuilt = prebuilt_dir(&ndk)?;
    let bin = prebuilt.join("bin");
    let sysroot = prebuilt.join("sysroot");
    let clang =
        util::tool_in(&bin, CLANG).ok_or_else(|| format!("no `{CLANG}` in {}", bin.display()))?;
    let ar = util::tool_in(&bin, "llvm-ar")
        .ok_or_else(|| format!("no `llvm-ar` in {}", bin.display()))?;
    let ranlib = util::tool_in(&bin, "llvm-ranlib")
        .ok_or_else(|| format!("no `llvm-ranlib` in {}", bin.display()))?;
    if !sysroot.join("usr/include").is_dir() {
        return Err(format!("no sysroot in {}", prebuilt.display()));
    }

    println!("ndk:       {}", ndk.display());
    println!("toolchain: {}", bin.display());
    println!("sysroot:   {}", sysroot.display());

    let toolchain = util::Clang {
        dir: &bin,
        clang: &clang,
        ar: &ar,
        ranlib: &ranlib,
        sysroot: &sysroot,
    };
    let mut envs = toolchain.env(TARGET, CLANG_TARGET);
    // `openssl-src` finds the NDK through this one as well.
    envs.push((
        "ANDROID_NDK_ROOT".to_string(),
        ndk.as_os_str().to_os_string(),
    ));

    util::cargo_build(TARGET, release, &envs)
}

/// `toolchains/llvm/prebuilt/<host>`, whatever the SDK calls the host this build runs on.
fn prebuilt_dir(ndk: &Path) -> Result<PathBuf, String> {
    let prebuilt = ndk.join("toolchains").join("llvm").join("prebuilt");
    let host = if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "darwin-arm64"
    } else if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else {
        "linux-x86_64"
    };

    let wanted = prebuilt.join(host);
    if wanted.is_dir() {
        return Ok(wanted);
    }
    let mut candidates: Vec<PathBuf> = fs::read_dir(&prebuilt)
        .map_err(|_| format!("no toolchain in {}", prebuilt.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| format!("no toolchain in {}", prebuilt.display()))
}

/// The newest `<sdk>/ndk/<version>` of an installed Android SDK.
fn ndk_from_sdk() -> Option<PathBuf> {
    let sdk = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .into_iter()
        .find_map(env::var_os)
        .map(PathBuf::from)
        .or_else(|| env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Android/Sdk")))
        .or_else(|| env::var_os("HOME").map(|dir| PathBuf::from(dir).join("Android/Sdk")))
        .or_else(|| {
            env::var_os("HOME").map(|dir| PathBuf::from(dir).join("Library/Android/sdk"))
        })?;

    let mut candidates: Vec<PathBuf> = fs::read_dir(sdk.join("ndk"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    candidates.sort();
    candidates.pop()
}
