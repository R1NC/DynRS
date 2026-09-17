//! Builds for `aarch64-apple-ios`.
//!
//! The C of the vendored libraries needs nothing from here: `cc` finds the SDK of an Apple target
//! with `xcrun` on its own (it adds `-isysroot`, `SDKROOT` and the deployment target), and the
//! `clang` that `libquickjs-ng-sys` hard codes is called with those flags as well.
//!
//! What does look at the host by default are the other two: bindgen parses the QuickJS headers
//! with libclang, which would read the macOS headers of the SDK-less host, and rustc links the
//! `cdylib` of `redb` with `cc`, which needs the target and the sysroot of iPhoneOS.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util;

const TARGET: &str = "aarch64-apple-ios";
const CLANG_TARGET: &str = "arm64-apple-ios";

pub fn build(sdk_root: Option<&Path>, release: bool) -> Result<(), String> {
    let sdk = match sdk_root {
        Some(path) => path.to_path_buf(),
        None => xcrun(&["--sdk", "iphoneos", "--show-sdk-path"])?,
    };
    if !sdk.join("usr").join("include").join("stdio.h").is_file() {
        return Err(format!("no iPhoneOS SDK in {}", sdk.display()));
    }
    let clang = xcrun(&["--sdk", "iphoneos", "--find", "clang"])?;
    if !clang.is_file() {
        return Err(format!(
            "no clang of the iPhoneOS toolchain at {}",
            clang.display()
        ));
    }

    let sdk = util::flag_path(&sdk);
    println!("sdk:   {sdk}");
    println!("clang: {}", clang.display());

    let envs = vec![
        // For the C that `cc` compiles, in case the target is not enough on its own.
        ("SDKROOT".to_string(), OsString::from(&sdk)),
        (
            "BINDGEN_EXTRA_CLANG_ARGS_aarch64_apple_ios".to_string(),
            format!("--target={CLANG_TARGET} -isysroot {sdk}").into(),
        ),
        (
            "CARGO_TARGET_AARCH64_APPLE_IOS_LINKER".to_string(),
            clang.as_os_str().to_os_string(),
        ),
        (
            "CARGO_TARGET_AARCH64_APPLE_IOS_RUSTFLAGS".to_string(),
            format!("-Clink-arg=--target={CLANG_TARGET} -Clink-arg=-isysroot -Clink-arg={sdk}")
                .into(),
        ),
    ];

    util::cargo_build(TARGET, release, &envs)
}

/// The path `xcrun` answers with, which is how the SDK and the clang of the toolchain are found.
fn xcrun(args: &[&str]) -> Result<PathBuf, String> {
    let output = Command::new("xcrun")
        .args(args)
        .output()
        .map_err(|error| format!("could not run `xcrun {}`: {error}", args.join(" ")))?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || path.is_empty() {
        return Err(format!(
            "`xcrun {}` gave no path; is Xcode installed?",
            args.join(" ")
        ));
    }
    Ok(PathBuf::from(path))
}
