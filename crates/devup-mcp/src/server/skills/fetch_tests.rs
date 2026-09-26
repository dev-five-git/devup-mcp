use super::*;
use crate::skills::{FetchedSkill, SkillFetchError, SkillUpstream};
use async_trait::async_trait;
use std::sync::Mutex;

struct FakeUpstream {
    result: Result<FetchedSkill, SkillFetchError>,
    urls: Mutex<Vec<String>>,
}

impl FakeUpstream {
    fn new(result: Result<FetchedSkill, SkillFetchError>) -> Self {
        Self {
            result,
            urls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl SkillUpstream for FakeUpstream {
    async fn fetch(&self, url: &str) -> Result<FetchedSkill, SkillFetchError> {
        self.urls.lock().unwrap().push(url.to_owned());
        self.result.clone()
    }
}

fn policy(label: &str) -> (PathBuf, super::super::output::OutputPolicy) {
    let project = std::env::temp_dir().join(format!(
        "devup-fetch-{label}-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&project).unwrap();
    let policy = super::super::output::OutputPolicy::from_roots(vec![project.clone()]).unwrap();
    (project, policy)
}

/// A scratch project with no machine-wide roots.
///
/// `project_only` is the point: reading the real home would make these
/// assertions depend on whether the developer running them happens to have
/// devup-ui installed in `~/.claude/skills`, which most of them do.
fn lookup(project: &std::path::Path) -> Lookup {
    Lookup::project_only(project.to_path_buf(), Runtime::ClaudeCode)
}

#[tokio::test]
async fn fetched_documents_report_their_real_provenance_and_manifest_url() {
    let (project, policy) = policy("fetched");
    let text = "---\nname: latest\n---\n# Current upstream rules\n";
    let upstream = FakeUpstream::new(Ok(FetchedSkill {
        contents: text.as_bytes().to_vec(),
        etag: "\"latest\"".to_owned(),
    }));
    let result = install_with(
        &policy,
        &lookup(&project),
        &["vespera".to_owned()],
        &upstream,
    )
    .await
    .unwrap();
    let written = &result["installed"][0];
    assert_eq!(written["source"], "fetched");
    assert!(written["reason"].is_null());
    assert!(result["warnings"].as_array().unwrap().is_empty());
    let url = "https://raw.githubusercontent.com/dev-five-git/vespera/HEAD/SKILL.md";
    assert_eq!(*upstream.urls.lock().unwrap(), vec![url]);
    let provenance = &written["documents"][0]["provenance"];
    assert_eq!(provenance["sourceUrl"], url);
    assert_eq!(provenance["etag"], "\"latest\"");
    assert!(provenance["fetchedAt"].as_u64().unwrap() > 0);
    assert_eq!(provenance["sha256"], crate::skills::sha256(text.as_bytes()));
    let body = std::fs::read_to_string(written["paths"][0].as_str().unwrap()).unwrap();
    assert!(body.starts_with("---\nname: latest\n---\n"));
    assert!(body.contains(&format!("Fetched from {url}")));
    assert!(body.contains(provenance["sha256"].as_str().unwrap()));
    assert!(body.contains(&provenance["fetchedAt"].to_string()));
    assert!(!body.contains("Vendored from"));
    assert!(body.ends_with("# Current upstream rules\n"));
    let again = install_with(
        &policy,
        &lookup(&project),
        &["vespera".to_owned()],
        &upstream,
    )
    .await
    .unwrap();
    assert_eq!(again["alreadyPresent"][0], "vespera");
    assert_eq!(upstream.urls.lock().unwrap().len(), 1);
    drop(policy);
    std::fs::remove_dir_all(project).unwrap();
}

#[tokio::test]
async fn every_fetch_failure_installs_embedded_but_only_404_warns() {
    for error in [
        SkillFetchError::Network("offline".to_owned()),
        SkillFetchError::HttpStatus(503),
        SkillFetchError::MissingEtag,
        SkillFetchError::NotFound,
    ] {
        let (project, policy) = policy("fallback");
        let upstream = FakeUpstream::new(Err(error.clone()));
        let result = install_with(
            &policy,
            &lookup(&project),
            &["devup-ui".to_owned()],
            &upstream,
        )
        .await
        .unwrap();
        let written = &result["installed"][0];
        assert_eq!(written["source"], "embedded");
        assert!(
            written["reason"]
                .as_str()
                .unwrap()
                .contains(&error.to_string())
        );
        let warnings = result["warnings"].as_array().unwrap();
        assert_eq!(
            warnings.len(),
            usize::from(error == SkillFetchError::NotFound)
        );
        if error == SkillFetchError::NotFound {
            assert!(
                warnings[0]["message"]
                    .as_str()
                    .unwrap()
                    .contains("manifest")
            );
            assert_eq!(warnings[0]["name"], "devup-ui");
            assert!(
                warnings[0]["sourceUrl"]
                    .as_str()
                    .unwrap()
                    .ends_with("/devup-ui/HEAD/SKILL.md")
            );
        }
        assert_eq!(
            std::fs::read_to_string(written["paths"][0].as_str().unwrap()).unwrap(),
            find_by_name("devup-ui").unwrap().document().unwrap()
        );
        drop(policy);
        std::fs::remove_dir_all(project).unwrap();
    }
}

#[tokio::test]
async fn own_external_and_unknown_skills_never_fetch() {
    let (project, policy) = policy("origins");
    let upstream = FakeUpstream::new(Err(SkillFetchError::NotFound));
    let result = install_with(
        &policy,
        &lookup(&project),
        &[
            "devfive-frontend".to_owned(),
            "vercel-react-best-practices".to_owned(),
        ],
        &upstream,
    )
    .await
    .unwrap();
    assert_eq!(result["installed"][0]["source"], "embedded");
    assert_eq!(result["installed"][0]["paths"].as_array().unwrap().len(), 5);
    assert_eq!(
        result["notInstallable"][0]["name"],
        "vercel-react-best-practices"
    );
    assert!(result["warnings"].as_array().unwrap().is_empty());
    assert!(
        install_with(
            &policy,
            &lookup(&project),
            &["unknown".to_owned()],
            &upstream
        )
        .await
        .is_err()
    );
    assert!(upstream.urls.lock().unwrap().is_empty());
    drop(policy);
    std::fs::remove_dir_all(project).unwrap();
}

#[tokio::test(start_paused = true)]
async fn the_entire_install_has_one_four_second_fetch_budget() {
    struct Stalled;
    #[async_trait]
    impl SkillUpstream for Stalled {
        async fn fetch(&self, _: &str) -> Result<FetchedSkill, SkillFetchError> {
            std::future::pending().await
        }
    }
    let (project, policy) = policy("timeout");
    let start = tokio::time::Instant::now();
    let result = install_with(&policy, &lookup(&project), &[], &Stalled)
        .await
        .unwrap();
    assert_eq!(start.elapsed(), std::time::Duration::from_secs(4));
    for written in result["installed"].as_array().unwrap() {
        assert_eq!(written["source"], "embedded");
    }
    assert!(result["warnings"].as_array().unwrap().is_empty());
    drop(policy);
    std::fs::remove_dir_all(project).unwrap();
}

/// The budget is for waiting on upstream. Writing what was fetched is local
/// work, and a slow disk used to spend the budget on it: on a slow Windows
/// runner the later skills of an install reported the budget elapsed - never
/// having asked upstream - instead of why they really could not be fetched.
#[tokio::test(start_paused = true)]
async fn time_spent_between_fetches_is_not_charged_to_the_fetch_budget() {
    let skill = find_by_name("devup-ui").unwrap();
    let offline = SkillFetchError::Network("DEVUP_MCP_SKILLS_OFFLINE disables fetching".to_owned());
    let upstream = FakeUpstream::new(Err(offline.clone()));
    let mut budget = FETCH_TIMEOUT;
    // Staging the skills before this one took longer than the whole budget.
    tokio::time::advance(FETCH_TIMEOUT * 2).await;
    let (_, error) = skill
        .fetch_documents(&upstream, &mut budget)
        .await
        .err()
        .unwrap();
    assert_eq!(error, offline);
    assert_eq!(upstream.urls.lock().unwrap().len(), 1, "upstream was asked");
    assert_eq!(
        budget, FETCH_TIMEOUT,
        "an answer that came at once cost nothing"
    );
}

#[tokio::test]
async fn nested_upstream_documents_are_fetched_whole_and_references_stay_byte_exact() {
    let mut record = find_by_name("devup-ui").unwrap().record.clone();
    record.path = "skills/example/SKILL.md".to_owned();
    let skill = Skill {
        record,
        uri: "test".to_owned(),
        resource_name: "test".to_owned(),
        texts: Some(&[
            ("SKILL.md", "# old entry"),
            ("references/rules.md", "old rules"),
        ]),
    };
    let upstream = FakeUpstream::new(Ok(FetchedSkill {
        contents: b"# current\n".to_vec(),
        etag: "current".to_owned(),
    }));
    let mut budget = FETCH_TIMEOUT;
    let result = skill.fetch_documents(&upstream, &mut budget).await.unwrap();
    assert_eq!(result.documents.len(), 2);
    assert!(result.documents[0].1.contains("Fetched from"));
    assert_eq!(result.documents[1].1, "# current\n");
    assert_eq!(
        *upstream.urls.lock().unwrap(),
        vec![
            "https://raw.githubusercontent.com/dev-five-git/devup-ui/HEAD/skills/example/SKILL.md",
            "https://raw.githubusercontent.com/dev-five-git/devup-ui/HEAD/skills/example/references/rules.md",
        ]
    );

    struct MissingReference;
    #[async_trait]
    impl SkillUpstream for MissingReference {
        async fn fetch(&self, url: &str) -> Result<FetchedSkill, SkillFetchError> {
            if url.ends_with("SKILL.md") {
                Ok(FetchedSkill {
                    contents: b"# new entry".to_vec(),
                    etag: "new".to_owned(),
                })
            } else {
                Err(SkillFetchError::NotFound)
            }
        }
    }
    // The caller receives an error, never a partial set to mix with the binary.
    let mut budget = FETCH_TIMEOUT;
    let failure = skill
        .fetch_documents(&MissingReference, &mut budget)
        .await
        .err()
        .unwrap();
    assert!(failure.0.ends_with("/references/rules.md"));
    assert_eq!(failure.1, SkillFetchError::NotFound);
}

#[tokio::test]
async fn a_failed_write_does_not_commit_any_skill() {
    let (project, policy) = policy("atomic");
    let blocked = project.join(".claude/skills/devfive-frontend/references");
    std::fs::create_dir_all(blocked.parent().unwrap()).unwrap();
    std::fs::write(&blocked, "not a directory").unwrap();
    let upstream = FakeUpstream::new(Ok(FetchedSkill {
        contents: b"# fetched".to_vec(),
        etag: "new".to_owned(),
    }));
    assert!(
        install_with(&policy, &lookup(&project), &[], &upstream)
            .await
            .is_err()
    );
    assert!(!project.join(".claude/skills/devup-ui/SKILL.md").exists());
    assert!(
        !project
            .join(".claude/skills/devfive-frontend/SKILL.md")
            .exists()
    );
    assert_eq!(std::fs::read_to_string(blocked).unwrap(), "not a directory");
    drop(policy);
    std::fs::remove_dir_all(project).unwrap();
}
