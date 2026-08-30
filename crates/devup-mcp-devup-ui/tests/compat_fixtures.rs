mod support;

use std::path::PathBuf;

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/devup-figma-plugin/cases")
        .join(relative)
}

fn collect_json(directory: &std::path::Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("fixture directory") {
        let path = entry.expect("fixture entry").path();
        if path.is_dir() {
            collect_json(&path, files);
        } else if path.extension().and_then(|value| value.to_str()) == Some("json") {
            files.push(path);
        }
    }
}

fn char_boundary_before(value: &str, index: usize) -> usize {
    let mut boundary = index.min(value.len());
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

#[test]
fn upstream_json_goldens() {
    let filter = std::env::var("DEVUP_FIXTURE_FILTER").ok();
    let root = fixture("");
    let mut files = Vec::new();
    collect_json(&root, &mut files);
    files.sort();
    assert_eq!(files.len(), 268);
    let mut failures = Vec::new();
    let mut passed = 0;
    for path in files {
        let case = support::load_case(&path).expect("upstream fixture");
        if filter.as_deref().is_some_and(|filter| {
            !case.id.contains(filter) && !case.source.test_id.contains(filter)
        }) {
            continue;
        }
        let actual = support::run_upstream_snapshot(&case).expect("upstream conversion");
        let category = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|value| value.to_str())
            .expect("fixture category");
        let snapshot = fixture("")
            .join("../snapshots")
            .join(category)
            .join(format!("{}.snap", case.id));
        let committed = std::fs::read_to_string(snapshot)
            .expect("committed snapshot")
            .replace("\r\n", "\n");
        let expected = committed
            .splitn(3, "---\n")
            .nth(2)
            .expect("insta snapshot body")
            .trim();
        if actual == expected {
            passed += 1;
        } else {
            let mismatch = expected
                .bytes()
                .zip(actual.bytes())
                .position(|(expected, actual)| expected != actual)
                .unwrap_or_else(|| expected.len().min(actual.len()));
            let start = char_boundary_before(expected, mismatch.saturating_sub(200));
            let actual_start = char_boundary_before(&actual, mismatch.saturating_sub(200));
            let expected_end = char_boundary_before(expected, expected.len().min(mismatch + 800));
            let actual_end = char_boundary_before(&actual, actual.len().min(mismatch + 800));
            failures.push(format!(
                "{} at byte {mismatch}: expected {:?}, actual {:?}",
                case.id,
                &expected[start..expected_end],
                &actual[actual_start..actual_end]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "upstream parity: {passed} passed, {} failed\n{}",
        failures.len(),
        failures
            .into_iter()
            .take(100)
            .collect::<Vec<_>>()
            .join("\n")
    );
}
