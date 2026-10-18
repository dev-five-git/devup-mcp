//! Source and compiled-in data checks run by the existing size-contract test.
use std::path::Path;

pub(super) fn check() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    visit(&workspace.join("crates"));
}
fn visit(dir: &Path) {
    for entry in std::fs::read_dir(dir).expect("read workspace source tree") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            visit(&path);
        } else if path.extension().is_some_and(|s| s == "rs") {
            let source = std::fs::read_to_string(&path).expect("read Rust source");
            let lines = source.lines().count();
            assert!(
                lines <= 1000,
                "{} has {lines} lines; maximum is 1000",
                path.display()
            );
        } else if path.extension().is_some_and(|s| s == "json")
            && path.components().any(|c| c.as_os_str() == "text")
        {
            let bytes = std::fs::read(&path).expect("read embedded JSON");
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        }
    }
}
