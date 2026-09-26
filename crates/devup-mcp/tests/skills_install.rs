//! The skill gap an agent can actually close.
//!
//! devup-mcp hands back devup-ui TSX. On a machine that has devup-mcp and
//! nothing else the receiving agent has never seen devup-ui, guesses, and this
//! server cannot see the guesses. So it reports the gap and installs what it
//! carries, and these tests hold that report to the disk it claims to describe.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ReadResourceRequestParams},
};
use serde_json::{Map, Value, json};

#[test]
fn self_check_works_without_entering_skill_delivery() -> anyhow::Result<()> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_devup-mcp"))
        .arg("--self-check")
        .env("DEVUP_MCP_SKILLS_OFFLINE", "0")
        .env("DEVUP_MCP_NO_UPDATE_CHECK", "1")
        .env("DEVUP_FIGMA_BRIDGE_PORT", "off")
        .output()?;
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["binary"], "ok");
    assert_eq!(report["serverConfig"], "ok");
    Ok(())
}

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("devup-skills-it-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch workspace");
    path
}

async fn session<F, T>(workspace: &Path, body: F) -> anyhow::Result<T>
where
    F: AsyncFnOnce(&rmcp::service::RunningService<rmcp::RoleClient, ()>) -> anyhow::Result<T>,
{
    // Set process configuration before startup, without racing other tests by
    // mutating this test process's environment. No HTTP or bridge socket opens.
    //
    // The home is pointed at an empty directory inside the scratch workspace.
    // Install state counts the machine-wide skill roots, because a skill in
    // `~/.codex/skills` really is loaded and really must not be reported
    // missing - but that makes the real home an input, and anyone working on
    // this repository has devup-ui installed in theirs. Left alone, these
    // assertions pass in CI and fail on the laptop that wrote them.
    let home = workspace.join("home");
    std::fs::create_dir_all(&home)?;
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_devup-mcp"))
        .current_dir(workspace)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("DEVUP_MCP_SKILLS_OFFLINE", "1")
        .env("DEVUP_MCP_NO_UPDATE_CHECK", "1")
        .env("DEVUP_FIGMA_BRIDGE_PORT", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let transport = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let client = ().serve(transport).await?;
    let out = body(&client).await;
    client.cancel().await?;
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait()).await??;
    assert!(status.success());
    out
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments))
        .await?;
    Ok(result.structured_content.expect("a structured response"))
}

fn skill<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["skills"]
        .as_array()
        .expect("skills array")
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("{name} is not in the report"))
}

/// How many skills this build carries and how many it only points at, read out
/// of the report rather than written down here.
///
/// These tests are about the install cycle, not about the size of the
/// registry. Spelling the totals out meant that adding one skill failed four
/// tests that had nothing to say about it.
fn carried_and_external(report: &Value) -> (usize, usize) {
    let skills = report["skills"].as_array().expect("skills array");
    let carried = skills
        .iter()
        .filter(|entry| entry["origin"] != "external")
        .count();
    (carried, skills.len() - carried)
}

/// The whole point, end to end: a bare workspace reports the gap, one call
/// closes it, and the state afterwards is read from the disk rather than
/// asserted by the call that did the writing.
#[tokio::test]
async fn a_bare_workspace_reports_the_gap_and_one_call_closes_it() -> anyhow::Result<()> {
    let workspace = scratch("cycle");
    let result = session(&workspace, async |client| {
        let before = call(client, "devup_skills", json!({"action": "status"})).await?;
        let (carried, external) = carried_and_external(&before);
        assert!(carried > 0 && external > 0, "{before}");
        assert_eq!(before["missingCount"], carried + external, "{before}");
        assert_eq!(before["installedCount"], 0);
        assert_eq!(skill(&before, "devup-ui")["installed"], false);

        let installed = call(client, "devup_skills", json!({"action": "install"})).await?;
        let after = installed["state"].clone();
        Ok((before, installed, after))
    })
    .await?;
    let (_, installed, after) = result;

    // Only what devup-mcp carries. The two vercel skills are someone else's
    // bytes and stay someone else's.
    let (carried, external) = carried_and_external(&after);
    let written = installed["installed"].as_array().unwrap();
    assert_eq!(written.len(), carried, "{installed}");
    for entry in written {
        assert_eq!(entry["source"], "embedded");
        if entry["name"] != "devfive-frontend" {
            assert!(
                entry["reason"]
                    .as_str()
                    .unwrap()
                    .contains("DEVUP_MCP_SKILLS_OFFLINE"),
                "{entry}"
            );
        }
        let paths = entry["paths"]
            .as_array()
            .unwrap_or_else(|| panic!("a skill reports every file it wrote: {entry}"));
        assert!(!paths.is_empty(), "{entry}");
        for path in paths {
            let path = Path::new(path.as_str().unwrap());
            assert!(
                path.is_file(),
                "{} was reported but not written",
                path.display()
            );
        }
        // The document a loader opens first has to say where it came from.
        // Which of the two notes it gets depends on the origin, and either one
        // answers the question a reader finding rules on disk actually has.
        let entry_document = paths
            .iter()
            .map(|path| Path::new(path.as_str().unwrap()))
            .find(|path| path.file_name().is_some_and(|name| name == "SKILL.md"))
            .unwrap_or_else(|| panic!("no SKILL.md among the written files: {entry}"));
        let body = std::fs::read_to_string(entry_document)?;
        // Asserted on the shape of the note, not on an organisation: a carried
        // skill does not have to be one of ours, and spelling `dev-five-git/`
        // here failed the first skill vendored from another org.
        assert!(
            body.contains("Vendored from ")
                || body.contains("Authored in ")
                || body.contains("Fetched from "),
            "an installed skill must carry its provenance: {}",
            entry_document.display()
        );
    }

    // A multi-document skill lands whole. A SKILL.md whose references are
    // missing is the shape that looks installed and whose rules are not there.
    let multi = written
        .iter()
        .find(|entry| entry["name"] == "devfive-frontend")
        .expect("devfive-frontend is carried");
    let paths = multi["paths"].as_array().unwrap();
    assert!(paths.len() > 1, "{multi}");
    assert!(
        paths
            .iter()
            .any(|path| path.as_str().unwrap().contains("references")),
        "the references have to be written too: {multi}"
    );

    assert_eq!(after["installedCount"], carried);
    assert_eq!(
        after["missingCount"], external,
        "the external skills remain"
    );
    assert_eq!(skill(&after, "devup-ui")["installed"], true);
    assert_eq!(
        skill(&after, "vercel-react-best-practices")["installed"],
        false
    );

    let _ = std::fs::remove_dir_all(&workspace);
    Ok(())
}

/// An external skill is reported, never written. Its publisher ships no
/// licence, so the bytes are not devup-mcp's to redistribute - and the command
/// that does install it is handed over rather than run.
#[tokio::test]
async fn external_skills_are_handed_over_as_a_command_and_never_written() -> anyhow::Result<()> {
    let workspace = scratch("external");
    session(&workspace, async |client| {
        let installed = call(
            client,
            "devup_skills",
            json!({"action": "install", "names": ["vercel-react-view-transitions"]}),
        )
        .await?;

        assert!(
            installed["installed"].as_array().unwrap().is_empty(),
            "nothing of theirs may be written: {installed}"
        );
        let refused = &installed["notInstallable"][0];
        assert_eq!(refused["name"], "vercel-react-view-transitions");
        assert_eq!(
            refused["command"],
            "npx skills add vercel-labs/agent-skills"
        );
        assert!(
            refused["why"].as_str().unwrap().contains("no LICENSE"),
            "declining to ship someone's work has to say why: {refused}"
        );
        assert!(
            installed["boundary"]
                .as_str()
                .unwrap()
                .contains("no install command was executed")
        );
        Ok(())
    })
    .await?;

    // The refusal is not a quiet no-op that left files behind.
    for root in [".claude/skills", ".opencode/skill", ".agents/skills"] {
        let path = workspace
            .join(root)
            .join("vercel-react-view-transitions")
            .join("SKILL.md");
        assert!(!path.exists(), "{} should not exist", path.display());
    }

    let _ = std::fs::remove_dir_all(&workspace);
    Ok(())
}

/// A workspace that already has a skill root has answered which runtime it is
/// for. Installing into a different one would write where nothing reads.
#[tokio::test]
async fn an_existing_skill_root_is_the_one_used() -> anyhow::Result<()> {
    let workspace = scratch("root");
    std::fs::create_dir_all(workspace.join(".opencode/skill"))?;

    session(&workspace, async |client| {
        let installed = call(
            client,
            "devup_skills",
            json!({"action": "install", "names": ["devup-ui"]}),
        )
        .await?;
        let path = installed["installed"][0]["paths"][0].as_str().unwrap();
        assert!(path.contains(".opencode"), "wrote to {path}");
        Ok(())
    })
    .await?;

    assert!(
        workspace
            .join(".opencode/skill/devup-ui/SKILL.md")
            .is_file()
    );
    assert!(
        !workspace.join(".claude/skills").exists(),
        "the default root must not be created when another already exists"
    );

    let _ = std::fs::remove_dir_all(&workspace);
    Ok(())
}

/// Reading a skill without installing it still has to work, and what comes back
/// has to say how old it is - a caller handed rules with no revision cannot
/// tell whether to trust them over the repository.
#[tokio::test]
async fn an_embedded_skill_is_readable_as_a_resource_with_its_provenance() -> anyhow::Result<()> {
    let workspace = scratch("resource");
    session(&workspace, async |client| {
        let listed = client.list_resources(Default::default()).await?;
        assert!(
            listed
                .resources
                .iter()
                .any(|resource| resource.uri == "devup://skill/devup-ui"),
            "the embedded skills must be listed"
        );
        assert!(
            !listed
                .resources
                .iter()
                .any(|resource| resource.uri.contains("vercel-")),
            "a URI must not be advertised for content this binary does not hold"
        );

        let read = client
            .read_resource(ReadResourceRequestParams::new("devup://skill/devup-ui"))
            .await?;
        let text = match &read.contents[0] {
            rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.clone(),
            other => panic!("a skill is text, got {other:?}"),
        };
        assert!(text.contains("Vendored from dev-five-git/devup-ui"));
        assert!(text.contains("Cannot run on the runtime"));
        Ok(())
    })
    .await?;

    let _ = std::fs::remove_dir_all(&workspace);
    Ok(())
}

/// A second install is not a second copy. Reporting an already-present skill as
/// missing would have the agent write a duplicate that then drifts.
#[tokio::test]
async fn installing_twice_changes_nothing_the_second_time() -> anyhow::Result<()> {
    let workspace = scratch("twice");
    session(&workspace, async |client| {
        let first = call(client, "devup_skills", json!({"action": "install"})).await?;
        let second = call(client, "devup_skills", json!({"action": "install"})).await?;
        let (carried, _) = carried_and_external(&second["state"]);

        assert_eq!(first["installed"].as_array().unwrap().len(), carried);
        assert!(
            second["installed"].as_array().unwrap().is_empty(),
            "{second}"
        );
        assert_eq!(
            second["alreadyPresent"].as_array().unwrap().len(),
            carried,
            "{second}"
        );
        assert_eq!(second["state"]["installedCount"], carried);
        Ok(())
    })
    .await?;

    let _ = std::fs::remove_dir_all(&workspace);
    Ok(())
}

/// An unknown name is a caller mistake worth naming, with the set that would
/// have worked - not a silent success that installs nothing.
#[tokio::test]
async fn an_unknown_skill_name_is_refused_with_the_known_set() -> anyhow::Result<()> {
    let workspace = scratch("unknown");
    session(&workspace, async |client| {
        let arguments: Map<String, Value> = json!({"action": "install", "names": ["react"]})
            .as_object()
            .cloned()
            .unwrap();
        let result = client
            .call_tool(
                CallToolRequestParams::new("devup_skills".to_owned()).with_arguments(arguments),
            )
            .await?;
        assert_eq!(result.is_error, Some(true), "{result:?}");
        let error = &result.structured_content.expect("a structured refusal")["error"];
        assert_eq!(error["code"], "DEVUP_INVALID_INPUT");
        assert!(error["message"].as_str().unwrap().contains("react"));
        // The set that would have worked, so the next call is a correction
        // rather than another guess.
        let known = error["details"]["known"].as_array().unwrap();
        let status = call(client, "devup_skills", json!({"action": "status"})).await?;
        let (carried, external) = carried_and_external(&status);
        assert_eq!(known.len(), carried + external, "{known:?}");
        assert!(known.iter().any(|name| name == "devup-ui"));
        assert!(known.iter().any(|name| name == "devfive-frontend"));
        Ok(())
    })
    .await?;

    let _ = std::fs::remove_dir_all(&workspace);
    Ok(())
}
