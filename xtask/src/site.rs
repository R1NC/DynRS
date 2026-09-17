//! Assembles the GitHub Pages site out of the reports and the API reference.
//!
//! The site is the landing page in `.github/pages/` plus the three directories it links to, laid
//! out the way it expects them. CI publishes the result; the same command builds it locally, which
//! is what makes the page reviewable before it goes out.

use std::fs;
use std::path::{Path, PathBuf};

use crate::util;

/// The landing page of the site, relative to the repository root.
const INDEX: &str = ".github/pages/index.html";

/// A part of the site: where it comes from, relative to the repository root, and the command that
/// has to be run first.
const COVERAGE: (&str, &str) = ("coverage/html", "cargo xtask report");
const TESTS: (&str, &str) = ("test-report.html", "cargo xtask report");
const DOCS: (&str, &str) = ("target/doc", "cargo doc --no-deps");

pub fn assemble(output: &Path) -> Result<(), String> {
    let root = util::repo_root();
    let index = root.join(INDEX);
    if !index.is_file() {
        return Err(format!("no landing page at {}", index.display()));
    }

    if output.exists() {
        fs::remove_dir_all(output)
            .map_err(|error| format!("could not clear {}: {error}", output.display()))?;
    }
    copy_file(&index, &output.join("index.html"))?;
    // `--output-dir coverage` puts the pages of the HTML report in `coverage/html`.
    copy_tree(&require(&root, COVERAGE)?, &output.join("coverage"))?;
    copy_file(&require(&root, TESTS)?, &output.join("tests/index.html"))?;
    // rustdoc writes the shared assets (`src/`, `static.files/`, the search index) next to the
    // crate directory, so the whole output goes in.
    copy_tree(&require(&root, DOCS)?, &output.join("docs"))?;

    println!("site: {}", output.display());
    Ok(())
}

/// The path of a part of the site, or the error that says what produces it.
fn require(root: &Path, part: (&str, &str)) -> Result<PathBuf, String> {
    let (relative, produced_by) = part;
    let path = root.join(relative);
    if path.exists() {
        Ok(path)
    } else {
        Err(format!("no `{relative}`; run `{produced_by}` first"))
    }
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        create_dir(parent)?;
    }
    fs::copy(from, to).map_err(|error| {
        format!(
            "could not copy {} to {}: {error}",
            from.display(),
            to.display()
        )
    })?;
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    create_dir(to)?;
    let entries = fs::read_dir(from)
        .map_err(|error| format!("could not read {}: {error}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read {}: {error}", from.display()))?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            copy_file(&entry.path(), &target)?;
        }
    }
    Ok(())
}

fn create_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))
}
