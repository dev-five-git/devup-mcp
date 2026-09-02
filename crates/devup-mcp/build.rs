use std::{env, path::PathBuf, process::Command};

mod build_identity;

use build_identity::{git_identity, safe};

fn main() {
    println!("cargo:rerun-if-env-changed=DEVUP_MCP_BUILD_ID");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_identity.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=src");
    for path in [git_path("HEAD"), git_path("index")].into_iter().flatten() {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let build_id = env::var("DEVUP_MCP_BUILD_ID")
        .ok()
        .filter(|value| safe(value))
        .or_else(git_build_id)
        .unwrap_or_else(|| "source-unknown".to_owned());
    println!("cargo:rustc-env=DEVUP_MCP_BUILD_ID={build_id}");
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
