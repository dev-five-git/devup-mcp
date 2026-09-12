use std::{env, path::PathBuf, process::Command};

mod build_identity;

use build_identity::{git_identity, safe};

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    println!("cargo:rustc-env=DEVUP_MCP_GIT_COMMIT={}", commit.trim());
    println!("cargo:rerun-if-env-changed=DEVUP_MCP_BUILD_ID");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_identity.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=src");
    for path in [git_path("HEAD"), git_path("index")].into_iter().flatten() {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let overridden = env::var("DEVUP_MCP_BUILD_ID")
        .ok()
        .filter(|value| safe(value));
    let source = if overridden.is_some() { "env" } else { "git" };
    let build_id = overridden
        .or_else(git_build_id)
        .unwrap_or_else(|| "source-unknown".to_owned());
    println!("cargo:rustc-env=DEVUP_MCP_BUILD_ID={build_id}");

    // What the working tree looked like AT BUILD TIME, and where the build id
    // came from. Without these, a test can only compare the baked `-dirty`
    // suffix against a fresh `git status`, which is a different observation at
    // a different time: an untracked file appearing after compilation - a probe
    // script, a scratch log - makes the binary say clean while the test's own
    // git call says dirty, and the test fails for a reason that is not a defect.
    // That happened. Recording the build-time facts lets the test check the
    // plumbing that can actually break (git observation -> `git_identity`
    // suffix -> `--version` output) without asserting that the tree stood still.
    println!(
        "cargo:rustc-env=DEVUP_MCP_BUILD_ID_SOURCE={}",
        if build_id == "source-unknown" {
            "unknown"
        } else {
            source
        }
    );
    println!(
        "cargo:rustc-env=DEVUP_MCP_GIT_DIRTY={}",
        match git_dirty() {
            Some(true) => "true",
            Some(false) => "false",
            None => "unknown",
        }
    );
}

/// Whether the working tree had changes when this build ran, or `None` when git
/// could not be asked. Separate from [`git_build_id`] so the observation can be
/// published on its own rather than only surviving as a `-dirty` suffix.
fn git_dirty() -> Option<bool> {
    let status = Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=normal"])
        .output()
        .ok()?;
    status.status.success().then_some(!status.stdout.is_empty())
}

fn git_build_id() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let status = Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=normal"])
        .output()
        .ok()?;
    if !status.status.success() {
        return None;
    }
    git_identity(Some(value.trim()), !status.stdout.is_empty())
}

fn git_path(name: &str) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-path", name])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    Some(PathBuf::from(path.trim()))
}
