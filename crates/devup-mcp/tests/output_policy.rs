use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use devup_mcp::server::output::OutputPolicy;

fn unique_temp_dir(label: &str) -> anyhow::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "devup-mcp-{label}-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir_all(&path)?;
    Ok(path)
}

#[test]
fn resolves_only_files_inside_preopened_roots() -> anyhow::Result<()> {
    let root = unique_temp_dir("allowed-root")?;
    let outside = unique_temp_dir("outside-root")?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;

    let relative = policy.resolve("nested/Component.tsx")?;
    assert_eq!(
        relative.display_path(),
        root.join("nested").join("Component.tsx")
    );
    let absolute_path = root.join("theme").join("devup.json");
    let absolute = policy.resolve(absolute_path.to_str().unwrap())?;
    assert_eq!(absolute.display_path(), absolute_path);

    for invalid in [
        "",
        ".",
        "../escape.tsx",
        root.to_str().unwrap(),
        outside.join("escape.tsx").to_str().unwrap(),
        "component.tsx:secret",
    ] {
        assert!(
            policy.resolve(invalid).is_err(),
            "accepted unsafe path: {invalid}"
        );
    }

    drop(relative);
    drop(absolute);
    drop(policy);
    fs::remove_dir_all(root)?;
    fs::remove_dir_all(outside)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_a_symlink_parent_that_escapes_the_root() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let root = unique_temp_dir("symlink-root")?;
    let outside = unique_temp_dir("symlink-outside")?;
    symlink(&outside, root.join("linked"))?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;

    assert!(policy.resolve("linked/escape.tsx").is_err());
    assert!(!outside.join("escape.tsx").exists());

    drop(policy);
    fs::remove_file(root.join("linked"))?;
    fs::remove_dir_all(root)?;
    fs::remove_dir_all(outside)?;
    Ok(())
}

#[cfg(windows)]
#[test]
fn rejects_a_directory_symlink_parent_that_escapes_the_root() -> anyhow::Result<()> {
    use std::os::windows::fs::symlink_dir;

    let root = unique_temp_dir("symlink-root")?;
    let outside = unique_temp_dir("symlink-outside")?;
    if let Err(error) = symlink_dir(&outside, root.join("linked")) {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            fs::remove_dir_all(root)?;
            fs::remove_dir_all(outside)?;
            return Ok(());
        }
        return Err(error.into());
    }
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;

    assert!(policy.resolve("linked/escape.tsx").is_err());
    assert!(!outside.join("escape.tsx").exists());

    drop(policy);
    fs::remove_dir(root.join("linked"))?;
    fs::remove_dir_all(root)?;
    fs::remove_dir_all(outside)?;
    Ok(())
}
