use std::{
    ffi::OsStr,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use cap_std::{ambient_authority, fs::Dir};
use devup_mcp_figma::{DevupError, ErrorCode};

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
    display_path: PathBuf,
}

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
        Ok(OutputTarget { display_path })
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
