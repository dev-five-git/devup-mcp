use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    SkillCommandConfig,
    server::output::{OutputPolicy, OutputTransaction},
};

mod upstream;

pub use upstream::{
    FetchedSkill, RawGithubSkillUpstream, SKILL_SOURCE_URL, SkillFetchError, SkillUpstream,
};

const SKILL_NAME: &str = "devup-ui";
const PROVENANCE_FILE: &str = "SKILL.md.provenance.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillReport {
    pub operation: &'static str,
    pub source_url: &'static str,
    pub targets: Vec<TargetReport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetReport {
    pub path: String,
    pub status: &'static str,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillProvenance {
    source_url: String,
    etag: String,
    fetched_at: u64,
    sha256: String,
}

pub async fn install(config: &SkillCommandConfig) -> anyhow::Result<SkillReport> {
    let (targets, mut skipped) = resolve_targets(&config.skill_dirs)?;
    if targets.is_empty() {
        let paths = skipped
            .iter()
            .map(|target| target.path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "No skill directory was detected. Skipped {paths} because their parent skill directories do not exist. Create one, or pass an existing directory with --skill-dir."
        );
    }
    let upstream = RawGithubSkillUpstream::new()?;
    let fetched = upstream.fetch().await?;
    if config.skill_dirs.is_empty() {
        for target in &targets {
            if !target.is_dir() {
                std::fs::create_dir(target).with_context(|| {
                    format!(
                        "Cannot create detected skill directory {}.",
                        target.display()
                    )
                })?;
            }
        }
    }
    let fetched_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("The system clock is before the Unix epoch.")?
        .as_secs();
    let mut report = install_fetched(fetched, &targets, fetched_at)?;
    report.targets.append(&mut skipped);
    Ok(report)
}

pub async fn check(config: &SkillCommandConfig) -> anyhow::Result<SkillReport> {
    let (targets, mut skipped) = resolve_targets(&config.skill_dirs)?;
    let mut report = check_with(&RawGithubSkillUpstream::new()?, &targets).await?;
    report.targets.append(&mut skipped);
    Ok(report)
}

pub async fn install_with(
    upstream: &dyn SkillUpstream,
    targets: &[PathBuf],
    fetched_at: u64,
) -> anyhow::Result<SkillReport> {
    let fetched = upstream.fetch().await?;
    install_fetched(fetched, targets, fetched_at)
}

fn install_fetched(
    fetched: FetchedSkill,
    targets: &[PathBuf],
    fetched_at: u64,
) -> anyhow::Result<SkillReport> {
    let provenance = SkillProvenance {
        source_url: SKILL_SOURCE_URL.to_owned(),
        etag: fetched.etag,
        fetched_at,
        sha256: sha256(&fetched.contents),
    };
    let mut provenance_bytes = serde_json::to_vec_pretty(&provenance)?;
    provenance_bytes.push(b'\n');
    let mut reports = Vec::with_capacity(targets.len());
    for target in targets {
        let policy = OutputPolicy::from_roots(vec![target.clone()]).map_err(|error| {
            anyhow::anyhow!(
                "Cannot install devup-ui skill into {}: {}",
                target.display(),
                error.message
            )
        })?;
        let mut transaction = OutputTransaction::new();
        transaction
            .stage("skill", policy.resolve("SKILL.md")?, &fetched.contents)
            .and_then(|()| {
                transaction.stage(
                    "provenance",
                    policy.resolve(PROVENANCE_FILE)?,
                    &provenance_bytes,
                )
            })
            .and_then(|()| transaction.commit().map(|_| ()))
            .map_err(|error| {
                anyhow::anyhow!(
                    "Cannot install devup-ui skill into {}: {}. Ensure the directory is writable and retry.",
                    target.display(),
                    error.message
                )
            })?;
        reports.push(target_report(target, "installed"));
    }
    Ok(SkillReport {
        operation: "install",
        source_url: SKILL_SOURCE_URL,
        targets: reports,
    })
}

pub async fn check_with(
    upstream: &dyn SkillUpstream,
    targets: &[PathBuf],
) -> anyhow::Result<SkillReport> {
    let fetched = upstream.fetch().await?;
    let upstream_hash = sha256(&fetched.contents);
    let targets = targets
        .iter()
        .map(|target| {
            let status = installed_status(target, &fetched.etag, &upstream_hash);
            target_report(target, status)
        })
        .collect();
    Ok(SkillReport {
        operation: "check",
        source_url: SKILL_SOURCE_URL,
        targets,
    })
}

fn installed_status(target: &Path, etag: &str, upstream_hash: &str) -> &'static str {
    let skill_path = target.join("SKILL.md");
    let Ok(contents) = std::fs::read(&skill_path) else {
        return "not-installed";
    };
    let provenance = std::fs::read(target.join(PROVENANCE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SkillProvenance>(&bytes).ok());
    provenance.map_or("stale", |provenance| {
        if provenance.source_url == SKILL_SOURCE_URL
            && provenance.etag == etag
            && provenance.sha256 == upstream_hash
            && sha256(&contents) == upstream_hash
        {
            "up-to-date"
        } else {
            "stale"
        }
    })
}

fn resolve_targets(explicit: &[PathBuf]) -> anyhow::Result<(Vec<PathBuf>, Vec<TargetReport>)> {
    if !explicit.is_empty() {
        return Ok((explicit.to_vec(), Vec::new()));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .context("Could not determine the home directory. Pass --skill-dir explicitly.")?;
    let roots = [
        home.join(".config/opencode/skill"),
        home.join(".claude/skills"),
    ];
    let mut targets = Vec::new();
    let mut skipped = Vec::new();
    for root in roots {
        if root.is_dir() {
            let target = root.join(SKILL_NAME);
            targets.push(target);
        } else {
            skipped.push(target_report(&root.join(SKILL_NAME), "skipped"));
        }
    }
    Ok((targets, skipped))
}

fn target_report(path: &Path, status: &'static str) -> TargetReport {
    TargetReport {
        path: path.to_string_lossy().into_owned(),
        status,
    }
}

fn sha256(contents: &[u8]) -> String {
    Sha256::digest(contents)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
