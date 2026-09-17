//! Helpers shared by the per-target builders.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The root of the repository, which is where cargo has to be run from.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("`xtask` has a directory of its own")
        .to_path_buf()
}

/// The `PATH` of this process with `dirs` in front of it.
///
/// The compiler is named explicitly (`CC_…` and `TARGET_CC`), but the `PATH` is still how an SDK
/// is found by the tools that look a name up on their own: bindgen for `libclang` and
/// `llvm-config`, and emscripten for its `emcc`/`em++`/`emar` shims.
pub fn path_with_front(dirs: &[&Path]) -> OsString {
    let separator = if cfg!(windows) { ";" } else { ":" };
    let mut value = OsString::new();
    for dir in dirs {
        value.push(dir.as_os_str());
        value.push(separator);
    }
    if let Some(existing) = env::var_os("PATH") {
        value.push(existing);
    }
    value
}

/// A path as it appears inside a compiler flag. The backslashes of Windows paths survive clang,
/// but not every tool that gets handed one, so they are turned into the separators that read the
/// same everywhere.
pub fn flag_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// `dir/name`, also accepting the suffixes the Windows packages use (`emcc.bat`, `.exe`, `.cmd`).
pub fn tool_in(dir: &Path, name: &str) -> Option<PathBuf> {
    let suffixes: &[&str] = if cfg!(windows) {
        &["", ".bat", ".cmd", ".exe"]
    } else {
        &[""]
    };
    suffixes
        .iter()
        .map(|suffix| dir.join(format!("{name}{suffix}")))
        .find(|path| path.is_file())
}

/// The clang toolchain of an SDK: the tools that compile the C and the sysroot whose headers go
/// with them.
pub struct Clang<'a> {
    /// The directory the tools live in, which goes in front of the `PATH`.
    pub dir: &'a Path,
    pub clang: &'a Path,
    pub ar: &'a Path,
    pub ranlib: &'a Path,
    pub sysroot: &'a Path,
}

impl Clang<'_> {
    /// The environment that hands this toolchain to every build script of `cargo_target`.
    ///
    /// `clang_target` is the triple clang itself is told, which on Android carries the API level
    /// as well. `cc` reads `CC_<target>`, but `libquickjs-ng-sys` hard codes `clang` and only
    /// reads `TARGET_CC`, so both are set; the target and the sysroot are repeated in `CFLAGS`
    /// and in the bindgen arguments because neither of those crates goes through the settings
    /// `cc` would take them from.
    pub fn env(&self, cargo_target: &str, clang_target: &str) -> Vec<(String, OsString)> {
        let flags = format!(
            "--target={clang_target} --sysroot={}",
            flag_path(self.sysroot)
        );
        let suffix = cargo_target.replace('-', "_");
        let cargo = cargo_target.to_uppercase().replace('-', "_");
        let clang = self.clang.as_os_str().to_os_string();
        let ar = self.ar.as_os_str().to_os_string();
        let ranlib = self.ranlib.as_os_str().to_os_string();

        vec![
            ("PATH".to_string(), path_with_front(&[self.dir])),
            ("TARGET_CC".to_string(), clang.clone()),
            (format!("CC_{suffix}"), clang),
            (format!("AR_{suffix}"), ar.clone()),
            (format!("RANLIB_{suffix}"), ranlib),
            (format!("CFLAGS_{suffix}"), flags.clone().into()),
            (format!("BINDGEN_EXTRA_CLANG_ARGS_{suffix}"), flags.into()),
            (format!("CARGO_TARGET_{cargo}_AR"), ar),
        ]
    }
}

/// The first file in `dir` whose name starts with `prefix`; the LLVM packages version their tools.
pub fn tool_like(dir: &Path, prefix: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(prefix))
        })
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}

/// The first file called `name` in the tree of `root`, so that an SDK is free to unpack into
/// whatever directory names it likes.
pub fn find_file(root: &Path, name: &str, max_depth: usize) -> Option<PathBuf> {
    find(root, max_depth, &|path| {
        path.is_file()
            && name_of(path).is_some_and(|value| {
                [".exe", ".cmd", ".bat"]
                    .into_iter()
                    .any(|suffix| value.strip_suffix(suffix) == Some(name))
                    || value == name
            })
    })
}

/// The first directory called `name` in the tree of `root`.
pub fn find_dir(root: &Path, name: &str, max_depth: usize) -> Option<PathBuf> {
    find(root, max_depth, &|path| {
        path.is_dir() && name_of(path).is_some_and(|value| value == name)
    })
}

fn name_of(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}

fn find(root: &Path, max_depth: usize, matches: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut level = vec![root.to_path_buf()];
    for _ in 0..max_depth {
        let mut next = Vec::new();
        for dir in level {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for path in entries.flatten().map(|entry| entry.path()) {
                if matches(&path) {
                    return Some(path);
                }
                if path.is_dir() {
                    next.push(path);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }
    None
}

/// Builds the library for `target` with the toolchain of `envs`.
pub fn cargo_build(target: &str, release: bool, envs: &[(String, OsString)]) -> Result<(), String> {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")));
    command
        .current_dir(repo_root())
        .args(["build", "--lib", "--locked", "--target", target]);
    if release {
        command.arg("--release");
    }
    for (key, value) in envs {
        command.env(key, value);
    }

    println!(
        "$ cargo build --lib --locked --target {target}{}",
        if release { " --release" } else { "" }
    );
    for (key, value) in envs.iter().filter(|(key, _)| key != "PATH") {
        println!("    {key}={}", value.to_string_lossy());
    }

    let status = command
        .status()
        .map_err(|error| format!("could not run cargo: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build failed ({status})"))
    }
}
