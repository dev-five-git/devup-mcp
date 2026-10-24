use super::*;

const FILES: [&str; 4] = ["project-plan.json", "screen-plan.json", "screen-plan.md", "units"];

pub(super) fn archive(root: &str, policy: &OutputPolicy) -> Result<String, DevupError> {
    let relative = format!(".devup/archive/{}-{}", now().replace([':', '.'], "-"), rand::random::<u64>());
    let base = Path::new(root).join(".devup");
    let destination = Path::new(root).join(&relative);
    for name in FILES {
        policy.resolve(&base.join(name).to_string_lossy())?;
        policy.resolve(&destination.join(name).to_string_lossy())?;
    }
    std::fs::create_dir_all(&destination).map_err(|error| invalid(error.to_string()))?;
    for name in FILES {
        let source = base.join(name);
        if source.exists() && let Err(error) = std::fs::rename(&source, destination.join(name)) {
            restore(root, &relative)?;
            return Err(invalid(format!("Archive project plan: {error}")));
        }
    }
    Ok(relative)
}

pub(super) fn restore(root: &str, archive: &str) -> Result<(), DevupError> {
    let destination = Path::new(root).join(".devup");
    for name in FILES.into_iter().rev() {
        let source = Path::new(root).join(archive).join(name);
        if source.exists() {
            std::fs::rename(source, destination.join(name)).map_err(|error| invalid(format!("Restore archived plan from {archive}: {error}")))?;
        }
    }
    Ok(())
}
