mod support;

use std::path::PathBuf;

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/devup-figma-plugin/cases")
        .join(relative)
}

#[test]
fn production_shaped_json_runs_through_the_converter() {
    let case = support::load_case(fixture("codegen/live-shape-smoke.json"))
        .expect("production-shaped fixture");
    assert_eq!(case.schema_version, 1);
    let output = support::run_case(&case).expect("fixture conversion");
    insta::with_settings!({
        snapshot_path => "../../../fixtures/devup-figma-plugin/snapshots/codegen",
        prepend_module_to_snapshot => false,
    }, {
        insta::assert_json_snapshot!(case.id, output);
    });
}
