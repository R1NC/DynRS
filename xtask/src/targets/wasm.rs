//! Builds for `wasm32-unknown-emscripten`.
//!
//! Two details of this target are easy to get wrong:
//!
//! * The C of the dependencies has to be compiled by the clang of the SDK. The headers of its
//!   sysroot lean on `__INT64_C`/`__UINT64_C`, which only became predefined macros in LLVM 20, so
//!   an older host clang cannot expand `UINT64_C`. `libquickjs-ng-sys` calls `cc` with `clang`
//!   hard coded and ignores `CC`, but it reads `TARGET_CC`, which is what names the SDK clang
//!   here; the toolchain still goes in front of the `PATH` for the other build scripts.
//! * A `.wasm` module is a `cdylib`, and rustc links it as a *side* module, so every object of the
//!   vendored C has to be position independent (hence `-fPIC`). `lua-src` compiles Lua as C++ on
//!   Emscripten, so the flags are needed in `CXXFLAGS` as well.
//! * clang gives a declaration hidden visibility unless the header marks it on this target, and
//!   `bindgen` skips whatever is not default visible, so the bindings lose every function whose
//!   header does not mark it. `libquickjs-ng-sys` 0.13 marks its API only when the header is built
//!   as a shared library, so its bindings would be empty of functions; see the `-fvisibility`
//!   argument below.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::util;

const TARGET: &str = "wasm32-unknown-emscripten";

pub fn build(sdk_root: Option<&Path>, release: bool) -> Result<(), String> {
    let emsdk = sdk_root
        .map(Path::to_path_buf)
        .or_else(|| env::var_os("EMSDK").map(PathBuf::from))
        .or_else(emcc_parent)
        .ok_or("no Emscripten SDK was found; activate emsdk or pass `--sdk-root`")?;

    let system = emsdk.join("upstream").join("emscripten");
    let toolchain = emsdk.join("upstream").join("bin");
    for tool in ["emcc", "em++", "emar"] {
        if util::tool_in(&system, tool).is_none() {
            return Err(format!("`{tool}` is not in {}", system.display()));
        }
    }
    let clang = util::tool_in(&toolchain, "clang").ok_or_else(|| {
        format!(
            "no `clang` in {}, so the C would be built by the host one",
            toolchain.display()
        )
    })?;

    let sysroot = ["cache/sysroot", "system"]
        .into_iter()
        .map(|name| system.join(name))
        .find(|candidate| candidate.join("include/stdio.h").is_file())
        .or_else(|| util::find_dir(&system, "sysroot", 4))
        .ok_or_else(|| format!("no sysroot was found under {}", system.display()))?;

    let sysroot_arg = format!("--sysroot={}", util::flag_path(&sysroot));
    println!("emscripten: {}", system.display());
    println!("toolchain:  {}", toolchain.display());
    println!("sysroot:    {}", sysroot.display());

    let envs = vec![
        (
            "PATH".to_string(),
            util::path_with_front(&[&system, &toolchain]),
        ),
        // `libquickjs-ng-sys` hard codes `clang` unless `TARGET_CC` names the compiler itself,
        // which is the point of this target: only the clang of the SDK knows `__INT64_C`.
        ("TARGET_CC".to_string(), clang.as_os_str().to_os_string()),
        ("CC_wasm32_unknown_emscripten".to_string(), "emcc".into()),
        ("CXX_wasm32_unknown_emscripten".to_string(), "em++".into()),
        ("AR_wasm32_unknown_emscripten".to_string(), "emar".into()),
        (
            "CFLAGS_wasm32_unknown_emscripten".to_string(),
            format!("{sysroot_arg} -fPIC").into(),
        ),
        (
            "CXXFLAGS_wasm32_unknown_emscripten".to_string(),
            format!("{sysroot_arg} -fPIC").into(),
        ),
        // `bindgen` asks clang for the API of the headers, and clang hides every declaration this
        // target does not see marked (`JS_EXTERN` expands to nothing unless the header is built as
        // a shared library). `bindgen` then drops the hidden declarations, so `-fvisibility=default`
        // is what leaves it with the same API the other targets see.
        (
            "BINDGEN_EXTRA_CLANG_ARGS_wasm32_unknown_emscripten".to_string(),
            OsString::from(format!("{sysroot_arg} -fvisibility=default")),
        ),
    ];

    // The manifest ships a static library for every other target. Asking cargo for the crate type
    // on the command line is not an option: `cargo rustc --crate-type` applies it to the
    // dependencies as well, and Emscripten then links each of them as its own side module.
    let manifest = util::repo_root().join("Cargo.toml");
    let original = fs::read_to_string(&manifest)
        .map_err(|error| format!("could not read {}: {error}", manifest.display()))?;
    let wanted = original.replace(CRATE_TYPE_STATIC, CRATE_TYPE_CDYLIB);
    if wanted == original {
        return Err(format!(
            "found no `{CRATE_TYPE_STATIC}` to replace in {}",
            manifest.display()
        ));
    }

    fs::write(&manifest, wanted)
        .map_err(|error| format!("could not write {}: {error}", manifest.display()))?;
    let result = util::cargo_build(TARGET, release, &envs);
    fs::write(&manifest, original)
        .map_err(|error| format!("could not restore {}: {error}", manifest.display()))?;
    result
}

const CRATE_TYPE_STATIC: &str = "crate-type = [\"staticlib\"]";
const CRATE_TYPE_CDYLIB: &str = "crate-type = [\"cdylib\"]";

/// Looks for `emcc` on the `PATH`, which is where an activated emsdk puts it, and takes the
/// `<emsdk>` around it (`<emsdk>/upstream/emscripten/emcc`).
fn emcc_parent() -> Option<PathBuf> {
    let emcc =
        env::split_paths(&env::var_os("PATH")?).find_map(|dir| util::tool_in(&dir, "emcc"))?;
    emcc.ancestors().nth(3).map(Path::to_path_buf)
}
