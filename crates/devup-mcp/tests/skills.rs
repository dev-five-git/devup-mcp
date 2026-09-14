use std::path::{Path, PathBuf};

use async_trait::async_trait;
use devup_mcp::skills::{FetchedSkill, SkillFetchError, SkillUpstream, check_with, install_with};

struct ScopedTempDir(PathBuf);

impl ScopedTempDir {
    fn new(label: &str) -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "devup-mcp-skills-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScopedTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct FakeUpstream(Result<FetchedSkill, SkillFetchError>);

#[async_trait]
impl SkillUpstream for FakeUpstream {
    async fn fetch(&self, source_url: &str) -> Result<FetchedSkill, SkillFetchError> {
        assert_eq!(source_url, devup_mcp::skills::SKILL_SOURCE_URL);
        self.0.clone()
    }
}

fn fetched(contents: &str, etag: &str) -> FetchedSkill {
    FetchedSkill {
        contents: contents.as_bytes().to_vec(),
        etag: etag.to_owned(),
    }
}

#[tokio::test]
async fn provenance_distinguishes_up_to_date_from_stale() -> anyhow::Result<()> {
    let target = ScopedTempDir::new("provenance")?;
    let first = FakeUpstream(Ok(fetched("# v1\n", "\"etag-v1\"")));
    let installed = install_with(&first, &[target.path().to_path_buf()], 1_789_171_200).await?;
    assert_eq!(installed.targets[0].status, "installed");

    let current = check_with(&first, &[target.path().to_path_buf()]).await?;
    assert_eq!(current.targets[0].status, "up-to-date");

    let changed = FakeUpstream(Ok(fetched("# v2\n", "\"etag-v2\"")));
    let stale = check_with(&changed, &[target.path().to_path_buf()]).await?;
    assert_eq!(stale.targets[0].status, "stale");

    let provenance: serde_json::Value = serde_json::from_slice(&std::fs::read(
        target.path().join("SKILL.md.provenance.json"),
    )?)?;
    assert_eq!(provenance["sourceUrl"], devup_mcp::skills::SKILL_SOURCE_URL);
    assert_eq!(provenance["etag"], "\"etag-v1\"");
    assert_eq!(provenance["fetchedAt"], 1_789_171_200_u64);
    Ok(())
}

#[tokio::test]
async fn network_failure_is_actionable_and_writes_nothing() -> anyhow::Result<()> {
    let target = ScopedTempDir::new("network")?;
    let upstream = FakeUpstream(Err(SkillFetchError::Network(
        "connection refused".to_owned(),
    )));

    let error = install_with(&upstream, &[target.path().to_path_buf()], 1)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("network"));
    assert!(!target.path().join("SKILL.md").exists());
    Ok(())
}

#[tokio::test]
async fn missing_upstream_skill_reports_404_and_writes_nothing() -> anyhow::Result<()> {
    let target = ScopedTempDir::new("not-found")?;
    let upstream = FakeUpstream(Err(SkillFetchError::NotFound));

    let error = install_with(&upstream, &[target.path().to_path_buf()], 1)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("404"));
    assert!(!target.path().join("SKILL.md").exists());
    Ok(())
}

#[tokio::test]
async fn non_writable_target_keeps_skill_and_provenance_uncommitted() -> anyhow::Result<()> {
    let target = ScopedTempDir::new("unwritable")?;
    std::fs::create_dir(target.path().join("SKILL.md"))?;
    let upstream = FakeUpstream(Ok(fetched("# skill\n", "\"etag\"")));

    let error = install_with(&upstream, &[target.path().to_path_buf()], 1)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("Cannot install devup-ui skill"));
    assert!(!target.path().join("SKILL.md.provenance.json").exists());
    Ok(())
}
