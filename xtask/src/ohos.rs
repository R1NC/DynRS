//! Builds for `aarch64-unknown-linux-ohos` with the HarmonyOS SDK.
//!
//! The SDK ships no OpenSSL, so the manifest builds it from source here as well. Its toolchain has
//! no target-prefixed wrapper either (unlike the NDK), so the clang that links has to be told the
//! target and the sysroot itself; a link happens even though the manifest ships a static library,
//! because `redb` declares a `cdylib` of its own.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::util;

const TARGET: &str = "aarch64-unknown-linux-ohos";
const FLAGS: &str = "--target=aarch64-linux-ohos";

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
    let toolchain = llvm_ar
        .parent()
        .expect("a file has a directory")
        .to_path_buf();
    let clang = util::tool_in(&toolchain, "clang")
        .or_else(|| util::tool_like(&toolchain, "clang-"))
        .ok_or_else(|| format!("no `clang` in {}", toolchain.display()))?;
    let ranlib = util::tool_in(&toolchain, "llvm-ranlib")
        .or_else(|| util::tool_in(&toolchain, "ranlib"))
        .ok_or_else(|| format!("no `llvm-ranlib` in {}", toolchain.display()))?;

    // The sysroot is next to the directory that holds the toolchain (`<sdk>/native/sysroot` for
    // `<sdk>/native/llvm/bin`), and some packages call it `sysroot_lite` instead. Only the one
    // with the C headers is usable.
    let sysroot = ["sysroot", "sysroot_lite"]
        .into_iter()
        .map(|name| toolchain.join(name))
        .find(|candidate| candidate.join("usr/include/stdio.h").is_file())
        .or_else(|| {
            toolchain
                .parent()
                .and_then(Path::parent)
                .and_then(|native| {
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

    let flags = format!("{FLAGS} --sysroot={}", util::flag_path(&sysroot));
    println!("sdk:       {}", sdk.display());
    println!("toolchain: {}", toolchain.display());
    println!("sysroot:   {}", sysroot.display());

    let envs = vec![
        ("PATH".to_string(), util::path_with_front(&[&toolchain])),
        (
            "CC_aarch64_unknown_linux_ohos".to_string(),
            clang.as_os_str().to_os_string(),
        ),
        (
            "AR_aarch64_unknown_linux_ohos".to_string(),
            llvm_ar.as_os_str().to_os_string(),
        ),
        (
            "RANLIB_aarch64_unknown_linux_ohos".to_string(),
            ranlib.as_os_str().to_os_string(),
        ),
        (
            "CFLAGS_aarch64_unknown_linux_ohos".to_string(),
            OsString::from(&flags),
        ),
        (
            "BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_linux_ohos".to_string(),
            OsString::from(flags),
        ),
        (
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER".to_string(),
            clang.as_os_str().to_os_string(),
        ),
        (
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_AR".to_string(),
            llvm_ar.as_os_str().to_os_string(),
        ),
        (
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_RUSTFLAGS".to_string(),
            format!(
                "-Clink-arg={FLAGS} -Clink-arg=--sysroot={}",
                util::flag_path(&sysroot)
            )
            .into(),
        ),
    ];

    util::cargo_build(TARGET, release, &envs)
}
