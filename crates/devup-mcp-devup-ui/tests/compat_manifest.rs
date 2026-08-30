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
