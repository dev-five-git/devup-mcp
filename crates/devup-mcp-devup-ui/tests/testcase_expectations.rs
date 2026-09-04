//! Compares generated code against the code the design itself carries.
//!
//! The devup-Test file states, next to each case, the devup-ui it is meant to
//! produce. That makes it ground truth of a kind the pinned corpus cannot be:
//! the corpus records what the plugin did, this records what the case is for.
//!
//! Captures live in `fixtures/local-screens/testcase-*.json` and are ignored by
//! git — with none present the test reports that and passes.
//!
//! It reports rather than asserts, and the reason matters. The stated code is
//! written by hand and describes the intent, not the output: against the
//! Gradient section every case differs, and in every one the pinned corpus
//! holds exactly what we emit — `-47deg` where the note reads `313deg`, `43%
//! 21%` where it reads `33.84% 33.84%`. Those are the same gradients said two
//! ways, and normalising toward the note would have broken three goldens and
//! moved away from the reference implementation.
//!
//! So a difference here is a question: check the corpus before treating it as
//! a defect. Where the corpus agrees with us the note is shorthand; where it
//! agrees with the note, that is ours to fix.

use std::{collections::BTreeMap, fs, path::PathBuf};

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;

/// The JSX inside `export function X() { return ( ... ) }`.
fn body(tsx: &str) -> String {
    let after_return = tsx.find("return (").map(|at| at + "return (".len());
    let start = after_return
        .and_then(|from| tsx[from..].find('<').map(|at| from + at))
        .unwrap_or(0);
    let end = tsx.rfind(");").unwrap_or(tsx.len());
    normalise(&tsx[start..end.max(start)])
}

/// Collapses the formatting so a comparison is about the code, not its layout.
fn normalise(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Case {
    expected: String,
    node_id: String,
}

fn cases(snapshot: &Snapshot) -> Vec<Case> {
    let node = |id: &str| snapshot.nodes.get(id);
    let x_of = |id: &str| {
        node(id)
            .and_then(|n| n.typed_view().number("x"))
            .unwrap_or(f64::MAX)
    };

    // A case frame holds the shape; the Code frame beside it holds the text.
    let mut roots: Vec<&String> = snapshot.roots.iter().collect();
    roots.sort_by(|left, right| x_of(left).total_cmp(&x_of(right)));

    let mut expectations: Vec<(f64, String)> = Vec::new();
    let mut shapes: Vec<(f64, String)> = Vec::new();
    for root in roots {
        let Some(raw) = node(root) else { continue };
        let view = raw.typed_view();
        let text = view
            .child_ids()
            .filter_map(|child| snapshot.nodes.get(child))
            .find_map(|child| {
                child
                    .typed_view()
                    .value("characters")
                    .and_then(|value| value.as_str())
                    .filter(|text| text.trim_start().starts_with('<'))
                    .map(str::to_owned)
            });
        match text {
            Some(text) => expectations.push((x_of(root), normalise(&text))),
            None => {
                if let Some(shape) = view.child_ids().next() {
                    shapes.push((x_of(root), shape.to_owned()));
                }
            }
        }
    }

    // Each expectation belongs to the nearest shape to its right.
    expectations
        .into_iter()
        .filter_map(|(at, expected)| {
            shapes
                .iter()
                .filter(|(shape_at, _)| *shape_at >= at)
                .min_by(|left, right| left.0.total_cmp(&right.0))
                .map(|(_, node_id)| Case {
                    expected,
                    node_id: node_id.clone(),
                })
        })
        .collect()
}

#[test]
fn generated_code_matches_what_each_case_states() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/local-screens");
    let Ok(entries) = fs::read_dir(&root) else {
        eprintln!("no captures; skipping");
        return;
    };

    let mut agreed = 0usize;
    let mut differed: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
        if !name.starts_with("testcase-") {
            continue;
        }
        let raw = fs::read_to_string(&path).expect("captured section");
        let value: serde_json::Value =
            serde_json::from_str(&raw).expect("captured section is json");
        let snapshot: Snapshot =
            serde_json::from_value(value["snapshot"].clone()).expect("captured snapshot");
        let label = value["label"].as_str().unwrap_or(name).to_owned();

        for case in cases(&snapshot) {
            let options = CodegenOptions {
                inline_instances: true,
                ..CodegenOptions::default()
            };
            let actual = match generate_component(&snapshot, &case.node_id, &options) {
                Ok(output) => body(&output.tsx),
                Err(error) => format!("<codegen failed: {error:?}>"),
            };
            if actual == case.expected {
                agreed += 1;
            } else {
                differed
                    .entry(label.clone())
                    .or_default()
                    .push((case.expected, actual));
            }
        }
    }

    let total = agreed + differed.values().map(Vec::len).sum::<usize>();
    if total == 0 {
        eprintln!("no captured test cases; skipping");
        return;
    }
    eprintln!("cases: {total}, matching what the design states: {agreed}");
    for (label, entries) in &differed {
        eprintln!("\n=== {label}");
        for (expected, actual) in entries {
            eprintln!("  states : {expected}");
            eprintln!("  we emit: {actual}\n");
        }
    }
}
