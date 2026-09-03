use std::{
    fs::{self, File, FileTimes},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use devup_mcp::server::output::{OutputPolicy, OutputTransaction};

/// Deliberately returns the spelling `std::env::temp_dir()` gives, symlinks and
/// all. On macOS that is under `/var/folders`, which resolves to
/// `/private/var/folders`, so configuring a policy from this exercises the case
/// where the configured root and the canonical root differ.
fn unique_temp_dir(label: &str) -> anyhow::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "devup-mcp-{label}-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir_all(&path)?;
    Ok(path)
}

/// Where the policy will actually report files, which is the canonical location
/// rather than the configured spelling. Assertions compare against this.
fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).expect("canonicalize an existing temp directory")
}

#[test]
fn resolves_only_files_inside_preopened_roots() -> anyhow::Result<()> {
    let root = unique_temp_dir("allowed-root")?;
    let outside = unique_temp_dir("outside-root")?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;

    let relative = policy.resolve("nested/Component.tsx")?;
    assert_eq!(
        relative.display_path(),
        canonical(&root).join("nested").join("Component.tsx")
    );
    // Spelled exactly as the root was configured, which on macOS is not the
    // canonical path. This must resolve, and must report the canonical one.
    let absolute_path = root.join("theme").join("devup.json");
    let absolute = policy.resolve(absolute_path.to_str().unwrap())?;
    assert_eq!(
        absolute.display_path(),
        canonical(&root).join("theme").join("devup.json")
    );

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

/// A root reached through a symlink is canonicalised when the policy opens it,
/// so the path devup-mcp reports back no longer shares a prefix with the one
/// the caller was given. Before this was handled, every such `outputPath` was
/// refused with "outputPath is outside the allowed root" — which on macOS is
/// not an edge case at all, since `/tmp` and `std::env::temp_dir()` both reach
/// their targets through `/var -> /private/var`.
///
/// Asserted here with an explicit symlink so the guarantee holds on every
/// platform with symlinks, instead of only where the OS happens to provide one.
#[cfg(unix)]
#[test]
fn accepts_a_root_reached_through_a_symlink_in_either_spelling() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let real = unique_temp_dir("symlink-spelling")?;
    let link = real.with_file_name(format!(
        "{}-link",
        real.file_name().unwrap().to_string_lossy()
    ));
    symlink(&real, &link)?;

    // Configured through the symlink, exactly as a client whose project path
    // traverses one would.
    let policy = OutputPolicy::from_roots(vec![link.clone()])?;
    let expected = canonical(&real).join("Component.tsx");

    let through_link = policy.resolve(link.join("Component.tsx").to_str().unwrap())?;
    assert_eq!(through_link.display_path(), expected);

    // The resolved spelling must keep working too.
    let through_real = policy.resolve(expected.to_str().unwrap())?;
    assert_eq!(through_real.display_path(), expected);

    // Accepting both spellings must not accept an escape through either.
    let outside = unique_temp_dir("symlink-spelling-outside")?;
    assert!(
        policy
            .resolve(outside.join("escape.tsx").to_str().unwrap())
            .is_err()
    );
    assert!(policy.resolve("../escape.tsx").is_err());

    drop(through_link);
    drop(through_real);
    drop(policy);
    fs::remove_file(&link)?;
    fs::remove_dir_all(real)?;
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

#[test]
fn commits_multiple_outputs_only_after_every_stage_succeeds() -> anyhow::Result<()> {
    let root = unique_temp_dir("transaction-success")?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let first = policy.resolve("Component.tsx")?;
    let second = policy.resolve("theme/devup.json")?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("tsx", first, b"export const Component = 1;\n")?;
    transaction.stage("devupJson", second, br#"{"theme":{}}"#)?;

    let paths = transaction.commit()?;

    assert_eq!(
        fs::read(root.join("Component.tsx"))?,
        b"export const Component = 1;\n"
    );
    assert_eq!(fs::read(root.join("theme/devup.json"))?, br#"{"theme":{}}"#);
    assert_eq!(
        paths["tsx"],
        canonical(&root).join("Component.tsx").to_string_lossy()
    );
    assert_eq!(
        paths["devupJson"],
        canonical(&root)
            .join("theme")
            .join("devup.json")
            .to_string_lossy()
    );

    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rejects_duplicate_targets_before_replacing_any_file() -> anyhow::Result<()> {
    let root = unique_temp_dir("transaction-duplicate")?;
    let original = root.join("same.txt");
    fs::write(&original, b"original")?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("first", policy.resolve("same.txt")?, b"first")?;

    let duplicate = transaction.stage("second", policy.resolve("same.txt")?, b"second");

    assert!(duplicate.is_err());
    assert_eq!(fs::read(&original)?, b"original");
    drop(transaction);
    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn staging_never_deletes_preexisting_internal_looking_siblings() -> anyhow::Result<()> {
    let root = unique_temp_dir("transaction-ttl-cleanup")?;
    let nested = root.join("nested");
    fs::create_dir_all(&nested)?;
    let expired_temp = nested.join(".devup-tmp-00000000000000000000000000000000");
    let expired_backup = nested.join(".devup-bak-11111111111111111111111111111111");
    let recent_temp = nested.join(".devup-tmp-22222222222222222222222222222222");
    let user_file = nested.join(".devup-tmp-user.txt");
    let other_directory = root.join("other");
    fs::create_dir_all(&other_directory)?;
    let untouched = other_directory.join(".devup-bak-33333333333333333333333333333333");
    for path in [
        &expired_temp,
        &expired_backup,
        &recent_temp,
        &user_file,
        &untouched,
    ] {
        fs::write(path, b"sentinel")?;
    }
    let old = SystemTime::now() - Duration::from_secs(25 * 60 * 60);
    for path in [&expired_temp, &expired_backup, &user_file, &untouched] {
        File::options()
            .write(true)
            .open(path)?
            .set_times(FileTimes::new().set_modified(old))?;
    }
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let mut transaction = OutputTransaction::new();

    transaction.stage("tsx", policy.resolve("nested/Component.tsx")?, b"new")?;

    assert!(expired_temp.exists());
    assert!(expired_backup.exists());
    assert!(recent_temp.exists());
    assert!(user_file.exists());
    assert!(untouched.exists());

    drop(transaction);
    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}
