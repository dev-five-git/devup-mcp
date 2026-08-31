mod support;

use std::path::PathBuf;

#[test]
fn pinned_plugin_corpus_is_complete_and_self_consistent() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devup-figma-plugin");
    let summary = support::validate_corpus(&root)
        .unwrap_or_else(|violations| panic!("compat corpus 위반:\n{}", violations.join("\n")));
    assert_eq!(summary.source_files, 54);
    assert_eq!(summary.ledger_entries, 978);
    assert_eq!(summary.cases, 268);
    assert_eq!(summary.snapshots, 268);
}

#[test]
fn manifest_hashes_are_stable_across_checkout_line_endings() {
    assert_eq!(
        support::hex_sha256(b"{\r\n  \"value\": true\r\n}\r\n"),
        support::hex_sha256(b"{\n  \"value\": true\n}\n")
    );
}

#[test]
fn coverage_registry_maps_every_inventory_entry_to_real_evidence() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devup-figma-plugin");
    let summary = support::validate_coverage_registry(&root)
        .unwrap_or_else(|violations| panic!("coverage registry 위반:\n{}", violations.join("\n")));

    assert_eq!(summary.inventory_entries, 978);
    assert_eq!(summary.snapshot_parity_entries, 252);
    assert_eq!(summary.snapshot_cases, 268);
    assert_eq!(summary.representative_assertion_entries, 666);
    assert_eq!(summary.non_parity_entries, 60);
    assert_eq!(
        summary.not_ported_entries, 0,
        "모든 upstream inventory 항목은 실행 evidence 또는 명시적인 범위 분류를 가져야 합니다."
    );
}
