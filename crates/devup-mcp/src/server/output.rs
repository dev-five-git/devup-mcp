use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use devup_mcp_figma::{DevupError, ErrorCode};
use rand::Rng;
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct OutputPolicy {
    roots: Arc<Vec<Arc<OutputRoot>>>,
}

struct OutputRoot {
    dir: Dir,
    display_path: PathBuf,
}

#[derive(Clone)]
pub struct OutputTarget {
    root: Arc<OutputRoot>,
    relative_path: PathBuf,
    display_path: PathBuf,
}

pub struct OutputTransaction {
    staged: Vec<StagedOutput>,
    targets: BTreeSet<PathBuf>,
}

struct StagedOutput {
    name: String,
    target: OutputTarget,
    temp_path: PathBuf,
    backup_path: Option<PathBuf>,
    backup_fingerprint: Option<FileFingerprint>,
    replaced: bool,
}

#[derive(Clone)]
struct FileFingerprint {
    bytes: u64,
    sha256: [u8; 32],
}

struct RollbackFailure {
    target_path: String,
    backup_path: Option<String>,
    operation: &'static str,
    message: String,
}

#[derive(Default)]
struct RollbackReport {
    failures: Vec<RollbackFailure>,
    recovery_paths: Vec<String>,
}

trait CommitHook {
    fn after_replacement(&mut self, _replaced: usize) -> std::io::Result<()> {
        Ok(())
    }

    fn before_restore(&mut self, _backup: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

struct NoopCommitHook;

impl CommitHook for NoopCommitHook {}

impl OutputPolicy {
    pub fn from_roots(roots: Vec<PathBuf>) -> Result<Self, DevupError> {
        if roots.is_empty() {
            return Err(invalid_path(
                "At least one allowed output root is required.",
            ));
        }
        let mut opened = Vec::with_capacity(roots.len());
        for root in roots {
            let display_path = dunce::canonicalize(&root).map_err(|error| {
                invalid_path(format!("Cannot resolve the output root: {error}"))
            })?;
            if !display_path.is_dir() {
                return Err(invalid_path(
                    "The output root must be an existing directory.",
                ));
            }
            let dir = Dir::open_ambient_dir(&display_path, ambient_authority())
                .map_err(|error| invalid_path(format!("Cannot open the output root: {error}")))?;
            opened.push(Arc::new(OutputRoot { dir, display_path }));
        }
        Ok(Self {
            roots: Arc::new(opened),
        })
    }

    pub fn resolve(&self, requested: &str) -> Result<OutputTarget, DevupError> {
        let path = Path::new(requested);
        if requested.trim().is_empty() {
            return Err(invalid_path("outputPath must be a file path."));
        }

        let (root, relative_path) = if path.is_absolute() {
            self.roots
                .iter()
                .find_map(|root| {
                    path.strip_prefix(&root.display_path)
                        .ok()
                        .map(|relative| (root.clone(), relative.to_path_buf()))
                })
                .ok_or_else(|| invalid_path("outputPath is outside the allowed root."))?
        } else {
            (self.roots[0].clone(), path.to_path_buf())
        };
        let relative_path = normalize_relative_file(&relative_path)?;
        reject_existing_symlink_ancestors(&root, &relative_path)?;
        let display_path = root.display_path.join(&relative_path);
        Ok(OutputTarget {
            root,
            relative_path,
            display_path,
        })
    }
}

impl OutputTransaction {
    pub fn new() -> Self {
        Self {
            staged: Vec::new(),
            targets: BTreeSet::new(),
        }
    }

    pub fn stage(
        &mut self,
        name: impl Into<String>,
        target: OutputTarget,
        contents: &[u8],
    ) -> Result<(), DevupError> {
        if !self.targets.insert(target.display_path.clone()) {
            return Err(invalid_path(
                "Two or more outputs cannot use the same file path.",
            ));
        }
        let parent = target
            .relative_path
            .parent()
            .unwrap_or_else(|| Path::new(""));
        target.root.dir.create_dir_all(parent).map_err(|error| {
            transaction_error(format!(
                "Cannot create the output parent directory: {error}"
            ))
        })?;
        reject_existing_symlink_ancestors(&target.root, &target.relative_path)?;
        let temp_path = unique_sibling(&target.relative_path, "tmp");
        let mut file = target
            .root
            .dir
            .open_with(&temp_path, OpenOptions::new().write(true).create_new(true))
            .map_err(|error| {
                transaction_error(format!("Cannot create the output staging file: {error}"))
            })?;
        if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = target.root.dir.remove_file(&temp_path);
            return Err(transaction_error(format!(
                "Cannot write the output staging file: {error}"
            )));
        }
        drop(file);
        self.staged.push(StagedOutput {
            name: name.into(),
            target,
            temp_path,
            backup_path: None,
            backup_fingerprint: None,
            replaced: false,
        });
        Ok(())
    }

    pub fn commit(mut self) -> Result<BTreeMap<String, String>, DevupError> {
        self.commit_with_hook(&mut NoopCommitHook)
    }

    fn commit_with_hook(
        &mut self,
        hook: &mut impl CommitHook,
    ) -> Result<BTreeMap<String, String>, DevupError> {
        for output in &self.staged {
            reject_existing_symlink_ancestors(&output.target.root, &output.target.relative_path)?;
            match output
                .target
                .root
                .dir
                .symlink_metadata(&output.target.relative_path)
            {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    return Err(transaction_error(
                        "The output target must be a regular file or not exist yet.",
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(transaction_error(format!(
                        "Cannot inspect the output target: {error}"
                    )));
                }
            }
        }

        let result = self.replace_all(hook);
        if let Err(error) = result {
            let rollback = self.rollback(hook);
            return if rollback.failures.is_empty() {
                Err(transaction_error(format!(
                    "The output transaction commit failed: {error}"
                )))
            } else {
                Err(transaction_rollback_error(error, rollback))
            };
        }

        for output in &mut self.staged {
            if let Some(backup) = output.backup_path.take() {
                let _ = output.target.root.dir.remove_file(backup);
            }
            output.backup_fingerprint = None;
        }
        Ok(self
            .staged
            .iter()
            .map(|output| {
                (
                    output.name.clone(),
                    output.target.display_path.to_string_lossy().into_owned(),
                )
            })
            .collect())
    }

    fn replace_all(&mut self, hook: &mut impl CommitHook) -> std::io::Result<()> {
        let mut replaced = 0;
        for output in &mut self.staged {
            if output
                .target
                .root
                .dir
                .symlink_metadata(&output.target.relative_path)
                .is_ok()
            {
                let backup_path = unique_sibling(&output.target.relative_path, "bak");
                let fingerprint = fingerprint(&output.target.root, &output.target.relative_path)?;
                output.target.root.dir.rename(
                    &output.target.relative_path,
                    &output.target.root.dir,
                    &backup_path,
                )?;
                output.backup_path = Some(backup_path);
                output.backup_fingerprint = Some(fingerprint);
            }
            output.target.root.dir.rename(
                &output.temp_path,
                &output.target.root.dir,
                &output.target.relative_path,
            )?;
            output.replaced = true;
            replaced += 1;
            hook.after_replacement(replaced)?;
        }
        Ok(())
    }

    fn rollback(&mut self, hook: &mut impl CommitHook) -> RollbackReport {
        let mut report = RollbackReport::default();
        for output in self.staged.iter_mut().rev() {
            let target_path = output.target.display_path.to_string_lossy().into_owned();
            let mut replacement_removed = true;
            if output.replaced {
                if let Err(error) = output
                    .target
                    .root
                    .dir
                    .remove_file(&output.target.relative_path)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    replacement_removed = false;
                    report.failures.push(RollbackFailure {
                        target_path: target_path.clone(),
                        backup_path: output.backup_path.as_ref().map(|backup| {
                            output
                                .target
                                .root
                                .display_path
                                .join(backup)
                                .to_string_lossy()
                                .into_owned()
                        }),
                        operation: "remove-replacement",
                        message: error.to_string(),
                    });
                }
                output.replaced = false;
            }
            if let Some(backup) = output.backup_path.take() {
                let backup_display = output.target.root.display_path.join(&backup);
                let backup_path = backup_display.to_string_lossy().into_owned();
                let restore = if replacement_removed {
                    hook.before_restore(&backup).and_then(|()| {
                        if let Some(expected) = output.backup_fingerprint.as_ref() {
                            verify_fingerprint(&output.target.root, &backup, expected)?;
                        }
                        restore_backup(
                            &output.target.root,
                            &backup,
                            &output.target.relative_path,
                            output.backup_fingerprint.as_ref(),
                        )
                    })
                } else {
                    Err(std::io::Error::other(
                        "Did not restore the backup because the replacement target could not be removed.",
                    ))
                };
                if let Err(error) = restore {
                    report.recovery_paths.push(backup_path.clone());
                    report.failures.push(RollbackFailure {
                        target_path: target_path.clone(),
                        backup_path: Some(backup_path),
                        operation: "restore-backup",
                        message: error.to_string(),
                    });
                }
                output.backup_fingerprint = None;
            }
            if let Err(error) = output.target.root.dir.remove_file(&output.temp_path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                report.failures.push(RollbackFailure {
                    target_path,
                    backup_path: None,
                    operation: "remove-staging",
                    message: error.to_string(),
                });
            }
        }
        report
    }
}

impl Default for OutputTransaction {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OutputTransaction {
    fn drop(&mut self) {
        for output in &mut self.staged {
            let _ = output.target.root.dir.remove_file(&output.temp_path);
            if let Some(backup) = output.backup_path.take() {
                let _ = output.target.root.dir.rename(
                    &backup,
                    &output.target.root.dir,
                    &output.target.relative_path,
                );
            }
            output.backup_fingerprint = None;
        }
    }
}

impl OutputTarget {
    pub fn display_path(&self) -> &Path {
        &self.display_path
    }
}

fn normalize_relative_file(path: &Path) -> Result<PathBuf, DevupError> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) if safe_component(value) => normalized.push(value),
            Component::CurDir => {}
            Component::Normal(_) => {
                return Err(invalid_path("outputPath contains an unsafe file name."));
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(invalid_path("outputPath cannot escape the allowed root."));
            }
        }
    }
    if normalized.as_os_str().is_empty() || normalized.file_name().is_none() {
        return Err(invalid_path("outputPath must be a file path."));
    }
    Ok(normalized)
}

fn safe_component(component: &OsStr) -> bool {
    let value = component.to_string_lossy();
    !value.is_empty() && !value.contains(':') && !value.contains('\0')
}

fn reject_existing_symlink_ancestors(
    root: &OutputRoot,
    relative_path: &Path,
) -> Result<(), DevupError> {
    let mut current = PathBuf::new();
    let parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
    for component in parent.components() {
        current.push(component.as_os_str());
        match root.dir.symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(invalid_path(
                    "A symlink or junction in an outputPath ancestor is not allowed.",
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(invalid_path("An outputPath ancestor is not a directory."));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(invalid_path(format!(
                    "Cannot inspect an outputPath ancestor: {error}"
                )));
            }
        }
    }
    Ok(())
}

fn invalid_path(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupCodegenFailed, message, false)
}

fn transaction_error(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupCodegenFailed, message, false)
}

fn transaction_rollback_error(
    commit_error: std::io::Error,
    rollback: RollbackReport,
) -> DevupError {
    let failures = rollback
        .failures
        .into_iter()
        .map(|failure| {
            json!({
                "targetPath": failure.target_path,
                "backupPath": failure.backup_path,
                "operation": failure.operation,
                "message": failure.message
            })
        })
        .collect::<Vec<_>>();
    DevupError::with_details(
        ErrorCode::DevupCodegenFailed,
        format!("The output transaction commit and rollback both failed: {commit_error}"),
        false,
        json!({
            "phase": "rollback",
            "commitError": commit_error.to_string(),
            "rollback": {
                "complete": false,
                "failures": failures,
                "recoveryPaths": rollback.recovery_paths
            }
        }),
    )
}

fn fingerprint(root: &OutputRoot, relative_path: &Path) -> std::io::Result<FileFingerprint> {
    let mut file = root.dir.open(relative_path)?;
    let bytes = file.metadata()?.len();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(FileFingerprint {
        bytes,
        sha256: hasher.finalize().into(),
    })
}

fn verify_fingerprint(
    root: &OutputRoot,
    relative_path: &Path,
    expected: &FileFingerprint,
) -> std::io::Result<()> {
    let observed = fingerprint(root, relative_path)?;
    if observed.bytes == expected.bytes && observed.sha256 == expected.sha256 {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "The restored output's length or hash does not match the original backup.",
        ))
    }
}

fn restore_backup(
    root: &OutputRoot,
    backup: &Path,
    target: &Path,
    expected: Option<&FileFingerprint>,
) -> std::io::Result<()> {
    let mut source = root.dir.open(backup)?;
    let permissions = source.metadata()?.permissions();
    let mut restored = root
        .dir
        .open_with(target, OpenOptions::new().write(true).create_new(true))?;
    let copy_result = std::io::copy(&mut source, &mut restored)
        .and_then(|_| restored.sync_all())
        .and_then(|_| root.dir.set_permissions(target, permissions));
    drop(restored);
    if let Err(error) = copy_result {
        let _ = root.dir.remove_file(target);
        return Err(error);
    }
    if let Some(expected) = expected
        && let Err(error) = verify_fingerprint(root, target, expected)
    {
        let _ = root.dir.remove_file(target);
        return Err(error);
    }
    root.dir.remove_file(backup)
}

fn unique_sibling(target: &Path, kind: &str) -> PathBuf {
    let mut random = [0_u8; 16];
    rand::rng().fill_bytes(&mut random);
    let suffix = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    target
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(format!(".devup-{kind}-{suffix}"))
}

#[cfg(test)]
mod tests {
    use std::{
        fs, io,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{CommitHook, OutputPolicy, OutputTransaction};

    struct FailAfterFirstReplacement;

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
        let path = std::env::temp_dir().join(format!(
            "devup-mcp-transaction-rollback-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&path)?;
        Ok(path)
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
}
