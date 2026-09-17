//! Renders the output of a test run as the HTML report that is published next to the coverage one.
//!
//! The coverage report of `cargo llvm-cov` says which lines the tests have reached, but not which
//! tests ran, so the log of the run is turned into a small page of its own.

use std::fs;
use std::path::Path;

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
        .map_err(|error| format!("could not write {}: {error}", output.display()))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
