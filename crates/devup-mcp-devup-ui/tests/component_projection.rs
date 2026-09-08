//! The same screen said twice, so a caller can place it in a project.
//!
//! `tsx` expands every instance into primitives: complete, but it cannot tell
//! you that a stretch of it is a Header the project may already own.
//! `componentTsx` keeps instances as `<Header />` with the import that resolves
//! them. Neither alone is enough — one cannot be split, the other cannot be
//! rendered — and the difference between them is each component's body, which
//! is what a caller writes into a new file when the component is missing.

use std::{fs, path::PathBuf};

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;

fn capture(name: &str) -> Option<Snapshot> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/local-screens")
        .join(name);
    let raw = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    serde_json::from_value(value.get("snapshot").cloned().unwrap_or(value)).ok()
}

fn render(snapshot: &Snapshot, root: &str, inline: bool) -> String {
    let options = CodegenOptions {
        inline_instances: inline,
        ..CodegenOptions::default()
    };
    generate_component(snapshot, root, &options)
        .expect("the capture converts")
        .tsx
}

#[test]
fn an_instance_is_a_reference_in_one_projection_and_its_parts_in_the_other() {
    let Some(snapshot) = capture("first-form.json") else {
        eprintln!("no capture; skipping");
        return;
    };
    let root = snapshot.roots.first().expect("a captured root");

    let expanded = render(&snapshot, root, true);
    let referenced = render(&snapshot, root, false);

    assert!(
        referenced.contains("<Header />"),
        "componentTsx should name the instance: {referenced}"
    );
    assert!(
        !expanded.contains("<Header />"),
        "tsx should have expanded it instead: {expanded}"
    );
    assert!(
        referenced.contains("from '@/components/Header'")
            || referenced.contains("from \"@/components/Header\""),
        "a named component needs the import that resolves it: {referenced}"
    );
}
