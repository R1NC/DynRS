//! Build helper for the targets whose C toolchain comes from an SDK.
//!
//! `cargo build --target …` cannot express what those targets need: the path of the SDK is only
//! known at run time, the vendored C reads `CC`/`CFLAGS`/`CXXFLAGS` from the environment, and the
//! `.cargo/config.toml` does not expand `${env.…}`. This binary locates the SDK, exports the
//! settings the crates expect and then runs cargo, so the CI workflows only have to install the
//! toolchain and call it.
//!
//! ```text
//! cargo xtask build --target ohos --release --sdk-root /path/to/ohos-sdk/linux
//! ```

mod android;
mod ohos;
mod util;
mod wasm;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
Builds a target whose C toolchain comes from an SDK.

Usage: cargo xtask build --target <wasm|android|ohos> [--release] [--sdk-root <path>]

  wasm     wasm32-unknown-emscripten  Emscripten, found through `EMSDK`
  android  aarch64-linux-android      the NDK, found through `ANDROID_NDK_HOME`,
                                      `ANDROID_NDK_ROOT`, `ANDROID_NDK_LATEST_HOME` or an
                                      `ndk/` directory of `ANDROID_HOME`/`ANDROID_SDK_ROOT`
  ohos     aarch64-unknown-linux-ohos the HarmonyOS SDK, found through `OHOS_SDK_ROOT`

`--sdk-root` overrides the search above. Every other target keeps `cargo build` as it was: the
static library and the `qjsc` tool.
";

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("build") => {}
        Some("help" | "--help" | "-h") | None => {
            print!("{USAGE}");
            return Ok(());
        }
        Some(other) => return Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }

    let (mut target, mut release, mut sdk_root) = (None, false, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--target" => target = Some(args.next().ok_or("`--target` needs a value")?),
            "--release" => release = true,
            "--sdk-root" => {
                sdk_root = Some(PathBuf::from(
                    args.next().ok_or("`--sdk-root` needs a value")?,
                ));
            }
            other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
        }
    }

    let target = target.ok_or("`--target` is required")?;
    let sdk_root = sdk_root.as_deref();
    match target.as_str() {
        "wasm" => wasm::build(sdk_root, release),
        "android" => android::build(sdk_root, release),
        "ohos" => ohos::build(sdk_root, release),
        other => Err(format!(
            "unknown target `{other}`, expected `wasm`, `android` or `ohos`"
        )),
    }
}
