//! A timed Smart Animate, against what the plugin wrote for it.
//!
//! `fixtures/plugin-answers/keyframes/pure.tsx` is the plugin's answer for
//! `devup-Test`'s frame `458:2021`, a loading spinner drawn as eight frames
//! that Smart-Animate to one another on a timer and back to the first. The
//! capture beside it, `fixtures/local-screens/keyframes.json`, holds all
//! eight because the snapshot script follows the chain; it is not committed,
//! so these skip when it is absent rather than pretending to have checked.

use std::{fs, path::PathBuf};

use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    ui_validate::{Severity, validate_devup_ui_tsx},
};
use devup_mcp_figma::{Snapshot, UpstreamResult};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn ours() -> Option<String> {
    let raw = fs::read_to_string(fixtures().join("local-screens/keyframes.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let snapshot: Snapshot = serde_json::from_value(value.get("snapshot")?.clone()).ok()?;
    let resource = |name: &str| {
        value
            .get("payload")
            .and_then(|payload| payload.get(name))
            .cloned()
            .map(|raw| UpstreamResult { raw })
    };
    let variables = resource("variables");
    let styles = resource("styles");
    let options =
        CodegenOptions::default().with_resource_results(variables.as_ref(), styles.as_ref());
    let root = snapshot.roots.first()?.clone();
    let output = generate_component(&snapshot, &root, &options).ok()?;
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "DEVUP_CODEGEN_ANIMATION_UNREACHABLE"),
        "every frame of the chain was collected: {:?}",
        output.diagnostics
    );
    Some(output.tsx)
}

/// A file's lines as they are compared: indentation and blank lines dropped,
/// and an expression that spans lines — `animationName={keyframes({...})}` —
/// folded onto one.
fn comparable_lines(source: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut open: Option<(String, i32)> = None;
    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let depth = line.chars().fold(0i32, |depth, character| match character {
            '{' | '[' | '(' => depth + 1,
            '}' | ']' | ')' => depth - 1,
            _ => depth,
        });
        match &mut open {
            Some((folded, balance)) => {
                folded.push(' ');
                folded.push_str(line);
                *balance += depth;
                if *balance <= 0 {
                    lines.push(folded.clone());
                    open = None;
                }
            }
            None => {
                if line.ends_with("({") && depth > 0 {
                    open = Some((line.to_owned(), depth));
                } else {
                    lines.push(line.to_owned());
                }
            }
        }
    }
    if let Some((folded, _)) = open {
        lines.push(folded);
    }
    lines
}

/// The lines on which this is written differently from the answer on
/// purpose. A 12px frame holding a 2px dot at its centre keeps its size; the
/// plugin writes none for a positioned frame with children, and the frame
/// collapses to the dot, which then lands 5px off. The 64px spinner frame,
/// a page root that places every dot absolutely, keeps its height for the
/// same reason: with nothing in flow the plugin's box has none. The answer
/// is the JSX alone, with no module around it.
const DIFFERS_ON_PURPOSE: &[(&str, &str)] = &[
    ("boxSize=\"12px\"", "a pinned size is a layout fact"),
    ("h=\"64px\"", "a pinned size is a layout fact"),
    (
        "import { Box, Flex, keyframes } from \"@devup-ui/react\";",
        "the answer is the JSX alone",
    ),
    ("export function _1() {", "the answer is the JSX alone"),
    ("return (", "the answer is the JSX alone"),
    (");", "the answer is the JSX alone"),
    ("}", "the answer is the JSX alone"),
];

/// The keyframes are the plugin's, to the percent and the pixel: what moves
/// from one frame of the chain to the next, at the moment it lands, the loop
/// closing at 100%, the duration counting the return, and no delay for a
/// timeout under 10ms.
#[test]
fn the_spinner_s_keyframes_are_the_plugin_s() {
    let Some(tsx) = ours() else {
        eprintln!("no keyframes capture; skipping");
        return;
    };
    let Ok(answer) = fs::read_to_string(fixtures().join("plugin-answers/keyframes/pure.tsx"))
    else {
        eprintln!("no keyframes answer; skipping");
        return;
    };
    let ours = comparable_lines(&tsx);
    let theirs = comparable_lines(&answer);
    let mut only_ours = ours.clone();
    for line in &theirs {
        if let Some(index) = only_ours.iter().position(|other| other == line) {
            only_ours.remove(index);
        }
    }
    let mut only_theirs = theirs.clone();
    for line in &ours {
        if let Some(index) = only_theirs.iter().position(|other| other == line) {
            only_theirs.remove(index);
        }
    }
    let unexplained = |lines: &[String]| {
        lines
            .iter()
            .filter(|line| !DIFFERS_ON_PURPOSE.iter().any(|(known, _)| known == line))
            .cloned()
            .collect::<Vec<_>>()
    };
    let ours_unexplained = unexplained(&only_ours);
    let theirs_unexplained = unexplained(&only_theirs);
    assert!(
        ours_unexplained.is_empty() && theirs_unexplained.is_empty(),
        "lines not accounted for.\n  written here and not in the answer:\n    {}\n  in the answer and not here:\n    {}",
        ours_unexplained.join("\n    "),
        theirs_unexplained.join("\n    ")
    );
    assert!(
        tsx.contains("animationName={keyframes({"),
        "the keyframes are a call, not a string:\n{tsx}"
    );
}

/// `keyframes({...})` holds only literals, and the animation props are ones
/// devup-ui takes; the validator the server offers must accept it.
#[test]
fn the_animated_module_passes_the_validator() {
    let Some(tsx) = ours() else {
        eprintln!("no keyframes capture; skipping");
        return;
    };
    let report = validate_devup_ui_tsx(&tsx, None, false);
    let errors = report
        .violations
        .iter()
        .filter(|violation| violation.severity == Severity::Error)
        .map(|violation| format!("{}: {}", violation.rule, violation.message))
        .collect::<Vec<_>>();
    assert!(report.ok && errors.is_empty(), "{}", errors.join("\n"));
}
