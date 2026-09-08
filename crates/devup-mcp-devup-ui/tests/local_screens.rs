//! Runs codegen over snapshots captured from a live Figma file.
//!
//! Figma meters tool calls, and one export spends about fifteen of them, so
//! checking a codegen change against a real screen used to cost allowance
//! every time — and ran out. These snapshots are captured once and replayed
//! for free, which is what makes it practical to see a change against real
//! designs rather than only synthetic nodes.
//!
//! They are scratch, not ground truth: the pinned corpus under
//! `fixtures/devup-figma-plugin` decides correctness, and this directory is
//! ignored by git. With nothing captured the test simply reports that and
//! passes, so a fresh checkout is never blocked on it.

use std::{fs, path::PathBuf};

use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    provenance::validate_fidelity,
};
use devup_mcp_figma::Snapshot;

fn captured() -> Vec<(String, String, Snapshot)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/local-screens");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut screens = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path).expect("captured screen");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("captured screen is json");
        let snapshot: Snapshot =
            serde_json::from_value(value["snapshot"].clone()).expect("captured snapshot");
        let label = value["label"].as_str().unwrap_or("screen").to_owned();
        let root_id = snapshot.roots.first().cloned().expect("a captured root");
        screens.push((label, root_id, snapshot));
    }
    screens
}

#[test]
fn every_captured_screen_converts_and_accounts_for_itself() {
    let screens = captured();
    if screens.is_empty() {
        eprintln!(
            "no captured screens in fixtures/local-screens; skipping. \
             Capture them from a live file to exercise this."
        );
        return;
    }

    let mut report = Vec::new();
    for (label, root_id, snapshot) in &screens {
        // Matches how the server converts a screen. Without inlining, an
        // instance stays a component reference and everything inside it goes
        // unemitted, which reads as a huge shortfall that the real path does
        // not have.
        let options = CodegenOptions {
            inline_instances: true,
            ..CodegenOptions::default()
        };
        let output = generate_component(snapshot, root_id, &options)
            .unwrap_or_else(|error| panic!("{label} failed to convert: {error:?}"));
        let fidelity = validate_fidelity(snapshot, root_id, &output)
            .unwrap_or_else(|error| panic!("{label} failed fidelity: {error:?}"));

        assert!(fidelity.syntax_valid, "{label} produced unparseable TSX");
        assert!(
            fidelity.uncovered_layout.is_empty(),
            "{label} leaves layout facts unaccounted for: {:?}",
            fidelity.uncovered_layout
        );
        assert_eq!(
            fidelity.text.covered, fidelity.text.total,
            "{label} dropped text"
        );

        report.push(format!(
            "  {label}: {} chars, layout {}/{}, text {}/{}",
            output.tsx.len(),
            fidelity.layout.covered,
            fidelity.layout.total,
            fidelity.text.covered,
            fidelity.text.total
        ));
    }

    eprintln!("captured screens:\n{}", report.join("\n"));
}
