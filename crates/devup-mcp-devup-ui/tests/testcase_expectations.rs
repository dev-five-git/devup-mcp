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
//! The size a case states is the same kind of shorthand. Every note here ends
//! `boxSize="150px"`, we emit nothing, and reading that as a defect and
//! restoring the size broke thirty-eight goldens — among them the very cases
//! being read. A shape on a page carries the canvas it was drawn on, not a size
//! anyone chose, and the plugin drops it; the note writes down what was drawn.
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
    let centre = |id: &str| {
        let view = snapshot.nodes.get(id)?.typed_view();
        let (x, y) = (view.number("x")?, view.number("y")?);
        let (w, h) = (
            view.number("width").unwrap_or(0.0),
            view.number("height").unwrap_or(0.0),
        );
        Some((x + w / 2.0, y + h / 2.0))
    };

    // A case frame holds the shape; a Code frame beside it holds the text that
    // says what the shape should produce.
    let mut expectations: Vec<((f64, f64), String)> = Vec::new();
    let mut shapes: Vec<((f64, f64), String)> = Vec::new();
    for root in &snapshot.roots {
        let Some(raw) = snapshot.nodes.get(root) else {
            continue;
        };
        let Some(at) = centre(root) else { continue };
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
            Some(text) => expectations.push((at, normalise(&text))),
            None => {
                // A case is sometimes wrapped in a frame that only positions it
                // and sometimes stands as the root itself, so neither the root
                // nor its first child is right on its own. A lone child is the
                // wrapped case; anything else is the case.
                let children = view.child_ids().collect::<Vec<_>>();
                let shape = match children.as_slice() {
                    [only] => (*only).to_owned(),
                    _ => root.clone(),
                };
                shapes.push((at, shape));
            }
        }
    }

    // Pair by proximity rather than by a fixed direction: a case sits above its
    // note in one section and beside it in another, so any rule about which way
    // to look holds for one layout and silently pairs nothing in the next.
    expectations
        .into_iter()
        .filter_map(|(at, expected)| {
            shapes
                .iter()
                .min_by(|left, right| {
                    let distance = |(x, y): (f64, f64)| (x - at.0).powi(2) + (y - at.1).powi(2);
                    distance(left.0).total_cmp(&distance(right.0))
                })
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
