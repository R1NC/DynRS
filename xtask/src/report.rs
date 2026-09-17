//! Runs the tests under `cargo llvm-cov`, and renders the HTML report of that run.
//!
//! The coverage report of `cargo llvm-cov` says which lines the tests have reached, but not which
//! tests ran, so the log of the run is turned into a small page of its own. Both are written where
//! the Pages site picks them up.

use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::util;

/// What a run writes, relative to the repository root: the coverage report, the log of the test
/// run, and the page rendered from it.
const COVERAGE: &str = "coverage";
const LOG: &str = "test-output.txt";
const REPORT: &str = "test-report.html";

/// Runs the tests with coverage and renders the unit test report of the same run. When
/// `fail_under` names a percentage of lines, a run that covers less fails as well.
pub fn run(fail_under: Option<f64>) -> Result<(), String> {
    let root = util::repo_root();
    let passed = coverage(&root)?;
    test_report(&root.join(LOG), &root.join(REPORT))?;
    if !passed {
        return Err("the tests failed".to_string());
    }
    if let Some(floor) = fail_under {
        let floor = floor.to_string();
        println!("$ cargo llvm-cov report --fail-under-lines {floor}");
        let met = llvm_cov(&root)
            .args(["report", "--fail-under-lines", &floor])
            .status()
            .map_err(|error| format!("could not run `cargo llvm-cov report`: {error}"))?
            .success();
        if !met {
            return Err(format!("line coverage is below {floor}%"));
        }
    }
    Ok(())
}

/// Runs `cargo llvm-cov`; answers whether the tests passed and both coverage files were written.
fn coverage(root: &Path) -> Result<bool, String> {
    println!("$ cargo llvm-cov --locked --html --output-dir {COVERAGE}");
    // Its stdout carries the `test …` lines the report is rendered from, so that stream is copied
    // into the log as it arrives; its stderr stays on the console, where the progress is.
    let mut child = llvm_cov(root)
        .args(["--html", "--output-dir", COVERAGE])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run `cargo llvm-cov`: {error} -- is it installed?"))?;

    let log = root.join(LOG);
    let mut file = File::create(&log)
        .map_err(|error| format!("could not write {}: {error}", log.display()))?;
    let output = child.stdout.take().expect("stdout was piped");
    for line in BufReader::new(output).lines() {
        let line =
            line.map_err(|error| format!("could not read what `cargo llvm-cov` printed: {error}"))?;
        println!("{line}");
        writeln!(file, "{line}")
            .map_err(|error| format!("could not write {}: {error}", log.display()))?;
    }
    let tests_passed = child
        .wait()
        .map_err(|error| format!("`cargo llvm-cov` did not finish: {error}"))?
        .success();

    // The lcov file is the machine readable half of the coverage, which CI uploads next to it.
    let lcov = format!("{COVERAGE}/lcov.info");
    println!("$ cargo llvm-cov report --lcov --output-path {lcov}");
    let reported = llvm_cov(root)
        .args(["report", "--lcov", "--output-path", &lcov])
        .status()
        .map_err(|error| format!("could not run `cargo llvm-cov report`: {error}"))?
        .success();

    Ok(tests_passed && reported)
}

fn llvm_cov(root: &Path) -> Command {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")));
    command.current_dir(root).args(["llvm-cov", "--locked"]);
    command
}

/// Renders `input`, the log of a test run, as the page at `output`.
pub fn test_report(input: &Path, output: &Path) -> Result<(), String> {
    let log = fs::read_to_string(input)
        .map_err(|error| format!("could not read {}: {error}", input.display()))?;

    // `cargo test` prints one `test <name> ... <result>` line per test, and one summary line per
    // test binary.
    let outcomes: Vec<(&str, &str)> = log
        .lines()
        .filter_map(|line| line.trim().strip_prefix("test "))
        .filter_map(|line| line.rsplit_once(" ... "))
        .map(|(name, result)| (name.trim(), result.trim()))
        .collect();
    let summaries: Vec<&str> = log
        .lines()
        .filter_map(|line| line.trim().strip_prefix("test result:"))
        .map(str::trim)
        .collect();
    let failed = outcomes
        .iter()
        .filter(|(_, result)| result.starts_with("FAILED"))
        .count();

    let mut html =
        String::from("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<title>DynRS unit tests</title>\n<style>\n");
    html.push_str(
        "  :root { color-scheme: light dark; }\n\
         \x20 body { margin: 0 auto; max-width: 56rem; padding: 2rem 1rem 4rem;\n\
         \x20        font: 16px/1.6 system-ui, -apple-system, \"Segoe UI\", sans-serif; }\n\
         \x20 table { border-collapse: collapse; width: 100%; }\n\
         \x20 th, td { border-bottom: 1px solid #8883; padding: .3rem .5rem; text-align: left; }\n\
         \x20 th { font-size: .8rem; letter-spacing: .1em; text-transform: uppercase; color: #666; }\n\
         \x20 td.result { white-space: nowrap; }\n\
         \x20 .ok { color: #2e7d32; }\n\
         \x20 .failed { color: #c62828; font-weight: 600; }\n\
         \x20 pre { overflow-x: auto; padding: 1rem; border: 1px solid #8883; border-radius: .3rem; }\n",
    );
    html.push_str("</style>\n</head>\n<body>\n<h1>DynRS unit tests</h1>\n");

    html.push_str(&format!(
        "<p class=\"{}\">{} tests, {} failed.</p>\n",
        if failed == 0 { "ok" } else { "failed" },
        outcomes.len(),
        failed
    ));
    if !summaries.is_empty() {
        html.push_str("<ul>\n");
        for summary in &summaries {
            html.push_str(&format!("<li>{}</li>\n", escape(summary)));
        }
        html.push_str("</ul>\n");
    }

    html.push_str("<table>\n<thead><tr><th>Test</th><th>Result</th></tr></thead>\n<tbody>\n");
    for (name, result) in &outcomes {
        let class = if result.starts_with("FAILED") {
            "failed"
        } else {
            "ok"
        };
        html.push_str(&format!(
            "<tr><td>{}</td><td class=\"result {class}\">{}</td></tr>\n",
            escape(name),
            escape(result)
        ));
    }
    html.push_str("</tbody>\n</table>\n");

    html.push_str("<details>\n<summary>Raw output</summary>\n<pre>");
    html.push_str(&escape(&log));
    html.push_str("</pre>\n</details>\n</body>\n</html>\n");

    fs::write(output, html)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;

    println!(
        "{} tests, {} failed -> {}",
        outcomes.len(),
        failed,
        output.display()
    );
    Ok(())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
