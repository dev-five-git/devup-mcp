use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use devup_mcp_figma::{DevupError, ErrorCode};
use rand::Rng;

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
    replaced: bool,
}

trait CommitHook {
    fn after_replacement(&mut self, _replaced: usize) -> std::io::Result<()> {
        Ok(())
    }
}

struct NoopCommitHook;

impl CommitHook for NoopCommitHook {}

impl OutputPolicy {
    pub fn from_roots(roots: Vec<PathBuf>) -> Result<Self, DevupError> {
        if roots.is_empty() {
            return Err(invalid_path("허용할 output root가 하나 이상 필요합니다."));
        }
        let mut opened = Vec::with_capacity(roots.len());
        for root in roots {
            let display_path = dunce::canonicalize(&root).map_err(|error| {
                invalid_path(format!("output root를 확인할 수 없습니다: {error}"))
            })?;
            if !display_path.is_dir() {
                return Err(invalid_path("output root는 존재하는 폴더여야 합니다."));
            }
            let dir = Dir::open_ambient_dir(&display_path, ambient_authority())
                .map_err(|error| invalid_path(format!("output root를 열 수 없습니다: {error}")))?;
            opened.push(Arc::new(OutputRoot { dir, display_path }));
        }
        Ok(Self {
            roots: Arc::new(opened),
        })
    }

    pub fn resolve(&self, requested: &str) -> Result<OutputTarget, DevupError> {
        let path = Path::new(requested);
        if requested.trim().is_empty() {
            return Err(invalid_path("outputPath는 파일 경로여야 합니다."));
        }

        let (root, relative_path) = if path.is_absolute() {
            self.roots
                .iter()
                .find_map(|root| {
                    path.strip_prefix(&root.display_path)
                        .ok()
                        .map(|relative| (root.clone(), relative.to_path_buf()))
                })
                .ok_or_else(|| invalid_path("outputPath가 허용된 root 밖에 있습니다."))?
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
                "둘 이상의 output이 같은 파일 경로를 사용할 수 없습니다.",
            ));
        }
        let parent = target
            .relative_path
            .parent()
            .unwrap_or_else(|| Path::new(""));
        target.root.dir.create_dir_all(parent).map_err(|error| {
            transaction_error(format!("output 상위 폴더를 만들 수 없습니다: {error}"))
        })?;
        reject_existing_symlink_ancestors(&target.root, &target.relative_path)?;
        let temp_path = unique_sibling(&target.relative_path, "tmp");
        let mut file = target
            .root
            .dir
            .open_with(&temp_path, OpenOptions::new().write(true).create_new(true))
            .map_err(|error| {
                transaction_error(format!("output staging 파일을 만들 수 없습니다: {error}"))
            })?;
        if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = target.root.dir.remove_file(&temp_path);
            return Err(transaction_error(format!(
                "output staging 파일을 기록할 수 없습니다: {error}"
            )));
        }
        drop(file);
        self.staged.push(StagedOutput {
            name: name.into(),
            target,
            temp_path,
            backup_path: None,
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
                        "output target은 일반 파일이거나 아직 존재하지 않아야 합니다.",
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(transaction_error(format!(
                        "output target을 확인할 수 없습니다: {error}"
                    )));
                }
            }
        }

        let result = self.replace_all(hook);
        if let Err(error) = result {
            self.rollback();
            return Err(transaction_error(format!(
                "output transaction commit에 실패했습니다: {error}"
            )));
        }

        for output in &mut self.staged {
            if let Some(backup) = output.backup_path.take() {
                let _ = output.target.root.dir.remove_file(backup);
            }
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
                output.target.root.dir.rename(
                    &output.target.relative_path,
                    &output.target.root.dir,
                    &backup_path,
                )?;
                output.backup_path = Some(backup_path);
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

    fn rollback(&mut self) {
        for output in self.staged.iter_mut().rev() {
            if output.replaced {
                let _ = output
                    .target
                    .root
                    .dir
                    .remove_file(&output.target.relative_path);
                output.replaced = false;
            }
            if let Some(backup) = output.backup_path.take() {
                let _ = output.target.root.dir.rename(
                    &backup,
                    &output.target.root.dir,
                    &output.target.relative_path,
                );
            }
            let _ = output.target.root.dir.remove_file(&output.temp_path);
        }
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
                return Err(invalid_path(
                    "outputPath에 안전하지 않은 파일명이 있습니다.",
                ));
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(invalid_path("outputPath는 허용 root를 벗어날 수 없습니다."));
            }
        }
    }
    if normalized.as_os_str().is_empty() || normalized.file_name().is_none() {
        return Err(invalid_path("outputPath는 파일 경로여야 합니다."));
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
                    "outputPath 상위 경로의 symlink 또는 junction은 허용하지 않습니다.",
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(invalid_path("outputPath 상위 경로가 폴더가 아닙니다."));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(invalid_path(format!(
                    "outputPath 상위 경로를 확인할 수 없습니다: {error}"
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
}
