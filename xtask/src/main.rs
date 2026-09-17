//! Repository automation: the cross builds, and the HTML test report.
//!
//! `cargo build --target …` cannot express what the cross targets need: the path of the SDK is only
//! known at run time, the vendored C reads `CC`/`CFLAGS`/`CXXFLAGS` from the environment, and the
//! `.cargo/config.toml` does not expand `${env.…}`. This binary locates the SDK, exports the
//! settings the crates expect and then runs cargo, so the CI workflows only have to install the
//! toolchain and call it.
//!
//! ```text
//! cargo xtask build --target ohos --release --sdk-root /path/to/ohos-sdk/linux
//! cargo xtask test-report --input test-output.txt --output test-report.html
//! ```

mod report;
mod site;
mod targets;
mod util;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
Builds a target whose C toolchain comes from an SDK, and renders the reports the site holds.

Usage: cargo xtask build --target <wasm|android|ohos|ios> [--release] [--sdk-root <path>]
       cargo xtask report
       cargo xtask site --output <dir>
       cargo xtask test-report --input <log> --output <html>

  wasm     wasm32-unknown-emscripten  Emscripten, found through `EMSDK`
  android  aarch64-linux-android      the NDK, found through `ANDROID_NDK_HOME`,
                                      `ANDROID_NDK_ROOT`, `ANDROID_NDK_LATEST_HOME` or an
                                      `ndk/` directory of `ANDROID_HOME`/`ANDROID_SDK_ROOT`
  ohos     aarch64-unknown-linux-ohos the HarmonyOS SDK, found through `OHOS_SDK_ROOT`
  ios      aarch64-apple-ios          the iPhoneOS SDK of Xcode, found through `xcrun`

`--sdk-root` overrides the search above. Every other target keeps `cargo build` as it was: the
static library and the `qjsc` tool.

`report` runs the tests under `cargo llvm-cov`, writing `coverage/` and `test-report.html`;
`--fail-under-lines <percent>` turns a coverage below that floor into a failure.
`site` lays those two, the API docs and the landing page out as the site to publish.
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
    let command = args.next();
    match command.as_deref() {
        Some("build") => build(args),
        Some("report") => report_command(args),
        Some("site") => site_command(args),
        Some("test-report") => test_report(args),
        Some("help" | "--help" | "-h") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}\n\n{USAGE}")),
    }
}

fn report_command(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut fail_under = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fail-under-lines" => {
                let value = args.next().ok_or("`--fail-under-lines` needs a value")?;
                fail_under = Some(
                    value
                        .parse::<f64>()
                        .map_err(|_| format!("`{value}` is not a percentage"))?,
                );
            }
            other => return Err(format!("unknown argument `{other}\n\n{USAGE}")),
        }
    }
    report::run(fail_under)
}

fn site_command(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => {
                output = Some(PathBuf::from(
                    args.next().ok_or("`--output` needs a value")?,
                ))
            }
            other => return Err(format!("unknown argument `{other}\n\n{USAGE}")),
        }
    }
    site::assemble(&output.ok_or("`--output` is required")?)
}

fn test_report(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let (mut input, mut output) = (None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => input = Some(PathBuf::from(args.next().ok_or("`--input` needs a value")?)),
            "--output" => {
                output = Some(PathBuf::from(
                    args.next().ok_or("`--output` needs a value")?,
                ))
            }
            other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
        }
    }
    let input = input.ok_or("`--input` is required")?;
    let output = output.ok_or("`--output` is required")?;
    report::test_report(&input, &output)
}

fn build(mut args: impl Iterator<Item = String>) -> Result<(), String> {
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
        "wasm" => targets::wasm::build(sdk_root, release),
        "android" => targets::android::build(sdk_root, release),
        "ohos" => targets::ohos::build(sdk_root, release),
        "ios" => targets::ios::build(sdk_root, release),
        other => Err(format!(
            "unknown target `{other}`, expected `wasm`, `android`, `ohos` or `ios`"
        )),
    }
}
