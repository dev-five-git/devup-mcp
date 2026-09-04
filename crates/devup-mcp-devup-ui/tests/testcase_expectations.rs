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

/// The generated JSX for one node, or nothing if it will not convert.
fn render(snapshot: &Snapshot, node_id: &str) -> Option<String> {
    let options = CodegenOptions {
        inline_instances: true,
        ..CodegenOptions::default()
    };
    generate_component(snapshot, node_id, &options)
        .ok()
        .map(|output| body(&output.tsx))
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
    let mut shapes: Vec<((f64, f64), String, Option<String>)> = Vec::new();
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
                // and sometimes stands as the root itself, and nothing about the
                // frame says which. The note does: one that opens a container
                // and puts something inside is describing the frame, one that is
                // a single element is describing what the frame holds. So keep
                // both readings and let the note pick.
                let lone_child = match view.child_ids().collect::<Vec<_>>().as_slice() {
                    [only] => Some((*only).to_owned()),
                    _ => None,
                };
                shapes.push((at, root.clone(), lone_child));
            }
        }
    }

    // Pair by proximity rather than by a fixed direction: a case sits above its
    // note in one section and beside it in another, so any rule about which way
    // to look holds for one layout and silently pairs nothing in the next.
    //
    // One note, one case. Letting each note take whatever is nearest lets them
    // crowd onto the same case, and a note whose case is far away claims the
    // commentary lying beside it instead — a difference reported against a
    // paragraph of Korean prose. Closest pairs are settled first, and each side
    // is spoken for once.
    // Distance alone still goes wrong, because a section holds more than its
    // cases: commentary explaining a rule, and frames kept alongside to show a
    // difference. Either can lie closer to a note than the case it describes,
    // and the comparison then reports a `<Box>` against a paragraph of prose.
    // What a note opens with says what it is describing, so a candidate that
    // starts the same way is preferred over one that merely sits closer.
    let opening_tag = |source: &str| {
        source
            .split_once('<')
            .map(|(_, rest)| {
                rest.trim_start_matches('/')
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            })
            .unwrap_or_default()
    };

    let mut pairs = Vec::with_capacity(expectations.len() * shapes.len());
    for (note, (at, expected)) in expectations.iter().enumerate() {
        let wanted = opening_tag(expected);
        for (case, (case_at, root, lone_child)) in shapes.iter().enumerate() {
            let describes_a_container = expected.matches('<').count() >= 3;
            let node_id = match lone_child {
                Some(child) if !describes_a_container => child,
                _ => root,
            };
            let same_kind = render(snapshot, node_id)
                .map(|rendered| opening_tag(&rendered) == wanted)
                .unwrap_or(false);
            let distance = (case_at.0 - at.0).powi(2) + (case_at.1 - at.1).powi(2);
            pairs.push((!same_kind, distance, note, case));
        }
    }
    pairs.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.total_cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.cmp(&right.3))
    });

    let mut spoken_for_note = vec![false; expectations.len()];
    let mut spoken_for_case = vec![false; shapes.len()];
    let mut cases = Vec::new();
    for (_, _, note, case) in pairs {
        if spoken_for_note[note] || spoken_for_case[case] {
            continue;
        }
        spoken_for_note[note] = true;
        spoken_for_case[case] = true;
        let expected = expectations[note].1.clone();
        // Three angle brackets means an element opened, something placed inside
        // it, and the element closed — a container. One or two is a single
        // element, with or without text of its own.
        let describes_a_container = expected.matches('<').count() >= 3;
        let (_, root, lone_child) = &shapes[case];
        let node_id = match lone_child {
            Some(child) if !describes_a_container => child.clone(),
            _ => root.clone(),
        };
        cases.push(Case { expected, node_id });
    }
    cases
}

#[test]
fn generated_code_matches_what_each_case_states() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/local-screens");
    let Ok(entries) = fs::read_dir(&root) else {
        eprintln!("no captures; skipping");
        return;
    };

    let mut agreed = 0usize;
    let mut differed: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
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
                differed.entry(label.clone()).or_default().push((
                    case.node_id.clone(),
                    case.expected,
                    actual,
                ));
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
        for (node_id, expected, actual) in entries {
            eprintln!("  node   : {node_id}");
            eprintln!("  states : {expected}");
            eprintln!("  we emit: {actual}\n");
        }
    }
}
