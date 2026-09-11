use std::path::{Path, PathBuf};

/// Canonicalize an existing root before joining expected output filenames.
/// Keep requested paths unresolved so tests still exercise macOS temp symlinks.
/// Do not normalize the returned value: it must already be canonical.
pub fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).expect("canonicalize an existing test directory")
}
