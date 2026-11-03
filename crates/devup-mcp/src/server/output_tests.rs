use std::{fs, io, path::PathBuf};

use super::{CommitHook, OutputPolicy, OutputTransaction};

struct FailAfterFirstReplacement;

struct EditSecond {
    path: PathBuf,
}
impl CommitHook for EditSecond {
    fn after_replacement(&mut self, replaced: usize) -> io::Result<()> {
        if replaced == 1 {
            fs::write(&self.path, b"concurrent second edit")?;
        }
        Ok(())
    }
}

#[test]
fn guarded_outputs_are_rechecked_during_commit_and_preserve_concurrent_changes()
-> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    let root = unique_root()?;
    fs::write(root.join("first"), b"old first")?;
    fs::write(root.join("second"), b"old second")?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let mut transaction = OutputTransaction::new();
    transaction.stage_registry(
        "first",
        policy.resolve("first")?,
        b"new first",
        Some(digest(b"old first")),
    )?;
    transaction.stage_registry(
        "second",
        policy.resolve("second")?,
        b"new second",
        Some(digest(b"old second")),
    )?;
    assert!(
        transaction
            .commit_with_hook(&mut EditSecond {
                path: root.join("second")
            })
            .is_err()
    );
    assert_eq!(fs::read(root.join("first"))?, b"old first");
    assert_eq!(fs::read(root.join("second"))?, b"concurrent second edit");
    drop(transaction);
    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}

struct EditThenFail {
    path: PathBuf,
}
impl CommitHook for EditThenFail {
    fn after_replacement(&mut self, _: usize) -> io::Result<()> {
        fs::write(&self.path, b"concurrent user edit")?;
        Err(io::Error::other("injected commit failure after edit"))
    }
}

#[test]
fn guarded_removal_is_restored_when_a_transaction_fails() -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    let root = unique_root()?;
    fs::write(root.join("legacy.png"), b"original")?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let digest = Sha256::digest(b"original")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut transaction = OutputTransaction::new();
    transaction.stage_remove("retire", policy.resolve("legacy.png")?, digest)?;
    assert!(
        transaction
            .commit_with_hook(&mut FailAfterFirstReplacement)
            .is_err()
    );
    assert_eq!(fs::read(root.join("legacy.png"))?, b"original");
    drop(transaction);
    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rollback_preserves_a_concurrent_edit_instead_of_deleting_it() -> anyhow::Result<()> {
    let root = unique_root()?;
    fs::write(root.join("source.tsx"), b"original source")?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("source", policy.resolve("source.tsx")?, b"new source")?;
    let error = transaction
        .commit_with_hook(&mut EditThenFail {
            path: root.join("source.tsx"),
        })
        .unwrap_err();
    assert_eq!(fs::read(root.join("source.tsx"))?, b"concurrent user edit");
    let recovery = error.details["rollback"]["recoveryPaths"][0]
        .as_str()
        .unwrap();
    assert_eq!(fs::read(recovery)?, b"original source");
    fs::remove_file(recovery)?;
    drop(transaction);
    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}

impl CommitHook for FailAfterFirstReplacement {
    fn after_replacement(&mut self, replaced: usize) -> io::Result<()> {
        if replaced == 1 {
            Err(io::Error::other("injected replacement failure"))
        } else {
            Ok(())
        }
    }
}

struct FailCommitAndRestore;

impl CommitHook for FailCommitAndRestore {
    fn after_replacement(&mut self, replaced: usize) -> io::Result<()> {
        if replaced == 1 {
            Err(io::Error::other("injected commit failure"))
        } else {
            Ok(())
        }
    }

    fn before_restore(&mut self, _backup: &std::path::Path) -> io::Result<()> {
        Err(io::Error::other("injected restore failure"))
    }
}

fn unique_root() -> anyhow::Result<PathBuf> {
    // Clock precision can make concurrent fixtures share a timestamp.
    // Reserve each root exclusively instead of reusing an existing directory.
    for _ in 0..32 {
        let path = std::env::temp_dir().join(format!(
            "devup-mcp-transaction-rollback-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "No exclusive fixture root available",
    )
    .into())
}

#[test]
fn concurrent_fixture_roots_remain_distinct() -> anyhow::Result<()> {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
    let workers = (0..16)
        .map(|_| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                (0..32)
                    .map(|_| {
                        barrier.wait();
                        unique_root()
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    let mut roots = std::collections::BTreeSet::new();
    for worker in workers {
        for root in worker.join().expect("fixture allocator worker panicked") {
            assert!(
                roots.insert(root?),
                "concurrent fixtures reused a directory"
            );
        }
    }
    assert_eq!(roots.len(), 512);
    for root in roots {
        fs::remove_dir(root)?;
    }
    Ok(())
}

#[test]
fn reports_canonical_paths_from_a_noncanonical_root_before_and_after_write() -> anyhow::Result<()> {
    let root = unique_root()?;
    fs::create_dir(root.join("child"))?;
    let alias = root.join("child").join("..");
    let canonical = crate::test_paths::canonical(&root);
    assert_ne!(alias, canonical);
    assert_eq!(crate::test_paths::canonical(&alias), canonical);

    let expected = canonical.join("nested").join("Component.tsx");
    assert!(!expected.exists());
    let policy = OutputPolicy::from_roots(vec![alias.clone()])?;
    // Alias parent components and traversal rejection are covered
    // separately in tests/output_policy.rs with explicit prefix fixtures.
    let absolute = policy.resolve(expected.to_str().unwrap())?;
    let relative = policy.resolve("nested/Component.tsx")?;
    assert_eq!(absolute.display_path(), expected);
    assert_eq!(relative.display_path(), expected);
    let mut transaction = OutputTransaction::new();
    transaction.stage("tsx", absolute, b"canonical output")?;
    let written = transaction.commit()?;
    assert_eq!(written["tsx"], expected.to_string_lossy());
    assert_eq!(fs::read(&expected)?, b"canonical output");
    drop(relative);
    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn restores_every_original_after_a_runtime_commit_failure() -> anyhow::Result<()> {
    let root = unique_root()?;
    fs::write(root.join("first.txt"), b"old-first")?;
    fs::write(root.join("second.txt"), b"old-second")?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("first", policy.resolve("first.txt")?, b"new-first")?;
    transaction.stage("second", policy.resolve("second.txt")?, b"new-second")?;

    let error = transaction
        .commit_with_hook(&mut FailAfterFirstReplacement)
        .expect_err("the injected failure must abort the transaction");

    assert!(error.message.contains("commit"));
    assert_eq!(fs::read(root.join("first.txt"))?, b"old-first");
    assert_eq!(fs::read(root.join("second.txt"))?, b"old-second");
    let internal_files = fs::read_dir(&root)?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(".devup-"))
        .collect::<Vec<_>>();
    assert!(internal_files.is_empty(), "stale files: {internal_files:?}");

    drop(transaction);
    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn w7_no_clobber_refuses_a_late_collision_and_rolls_back_published_files() -> anyhow::Result<()> {
    struct CreateCollision(PathBuf);
    impl CommitHook for CreateCollision {
        fn after_replacement(&mut self, replaced: usize) -> io::Result<()> {
            if replaced == 1 {
                fs::write(self.0.join("second.tsx"), b"concurrent original")?;
            }
            Ok(())
        }
    }
    let root = std::env::temp_dir().join(format!(
        "devup-w7-transaction-{:016x}",
        rand::random::<u64>()
    ));
    fs::create_dir(&root)?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let mut transaction = OutputTransaction::new();
    transaction.refuse_existing();
    transaction.stage("page", policy.resolve("first.tsx")?, b"new page")?;
    transaction.stage("screen", policy.resolve("second.tsx")?, b"new screen")?;
    let error = transaction
        .commit_with_hook(&mut CreateCollision(root.clone()))
        .unwrap_err();
    assert!(error.message.contains("second.tsx"));
    assert!(!root.join("first.tsx").exists());
    assert_eq!(fs::read(root.join("second.tsx"))?, b"concurrent original");
    drop(transaction);
    assert_eq!(fs::read_dir(&root)?.count(), 1);
    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn w7_later_scaffold_file_failure_restores_originals_in_reverse_order() -> anyhow::Result<()> {
    struct FailSecond(Vec<String>);
    impl CommitHook for FailSecond {
        fn after_replacement(&mut self, replaced: usize) -> io::Result<()> {
            if replaced == 2 {
                Err(io::Error::other("later scaffold failure"))
            } else {
                Ok(())
            }
        }
        fn before_restore(&mut self, backup: &std::path::Path) -> io::Result<()> {
            self.0.push(backup.to_string_lossy().into_owned());
            Ok(())
        }
    }
    let root = std::env::temp_dir().join(format!(
        "devup-w7-transaction-{:016x}",
        rand::random::<u64>()
    ));
    fs::create_dir(&root)?;
    let page = "src/app/proofread/page.tsx";
    let screen = "components/pages/proofread/Proofread.tsx";
    for name in [page, screen] {
        fs::create_dir_all(root.join(name).parent().unwrap())?;
        fs::write(root.join(name), name)?;
    }
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("pageScaffold:page", policy.resolve(page)?, b"new page")?;
    transaction.stage(
        "pageScaffold:component",
        policy.resolve(screen)?,
        b"new screen",
    )?;
    let mut hook = FailSecond(Vec::new());
    assert!(transaction.commit_with_hook(&mut hook).is_err());
    for name in [page, screen] {
        assert_eq!(fs::read_to_string(root.join(name))?, name);
    }
    assert!(hook.0[0].contains("components"));
    assert!(hook.0[1].contains("src"));
    drop(transaction);
    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn reports_and_retains_a_backup_when_rollback_restoration_fails() -> anyhow::Result<()> {
    let root = unique_root()?;
    fs::write(root.join("first.txt"), b"old-first")?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("first", policy.resolve("first.txt")?, b"new-first")?;

    let error = transaction
        .commit_with_hook(&mut FailCommitAndRestore)
        .expect_err("the injected rollback failure must be reported");

    assert_eq!(error.details["phase"], "rollback");
    assert_eq!(error.details["rollback"]["complete"], false);
    let recovery_path = error.details["rollback"]["recoveryPaths"][0]
        .as_str()
        .expect("retained backup path");
    assert!(std::path::Path::new(recovery_path).exists());
    assert_eq!(fs::read(recovery_path)?, b"old-first");
    assert!(!root.join("first.txt").exists());

    fs::remove_file(recovery_path)?;
    drop(transaction);
    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}
