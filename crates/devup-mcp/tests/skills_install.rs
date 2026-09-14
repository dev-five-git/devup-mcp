//! The skill gap an agent can actually close.
//!
//! devup-mcp hands back devup-ui TSX. On a machine that has devup-mcp and
//! nothing else the receiving agent has never seen devup-ui, guesses, and this
//! server cannot see the guesses. So it reports the gap and installs what it
//! carries, and these tests hold that report to the disk it claims to describe.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{AuthStatus, DevupError, FigmaUpstream, ReadToolCall, UpstreamResult};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ReadResourceRequestParams},
};
use serde_json::{Map, Value, json};

/// Nothing here reaches Figma. Installing a skill is a local write of bytes
/// already in the binary, and a test that needed a network to prove that would
/// be proving the wrong thing.
struct Offline;

#[async_trait]
impl DevupAuth for Offline {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
    async fn login(&self) -> Result<AuthStatus, DevupError> {
        panic!("installing a skill must never authenticate")
    }
    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
}

#[async_trait]
impl FigmaUpstream for Offline {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec![])
    }
    async fn call_read_tool(&self, _: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        panic!("installing a skill must never read Figma")
    }
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
    let server = DevupServer::with_output_roots(
        Services::new(Arc::new(Offline), Arc::new(Offline)),
        vec![workspace.to_path_buf()],
    )?;
    let (server_transport, client_transport) = tokio::io::duplex(512 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let out = body(&client).await;
    client.cancel().await?;
    let _ = task.await;
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

/// The whole point, end to end: a bare workspace reports the gap, one call
/// closes it, and the state afterwards is read from the disk rather than
/// asserted by the call that did the writing.
#[tokio::test]
async fn a_bare_workspace_reports_the_gap_and_one_call_closes_it() -> anyhow::Result<()> {
    let workspace = scratch("cycle");
    let result = session(&workspace, async |client| {
        let before = call(client, "devup_skills", json!({"action": "status"})).await?;
        assert_eq!(before["missingCount"], 5, "{before}");
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
    let written = installed["installed"].as_array().unwrap();
    assert_eq!(written.len(), 3, "{installed}");
    for entry in written {
        let path = Path::new(entry["path"].as_str().unwrap());
        assert!(
            path.is_file(),
            "{} was reported but not written",
            path.display()
        );
        let body = std::fs::read_to_string(path)?;
        assert!(
            body.contains("Vendored from dev-five-git/"),
            "an installed skill must carry the revision it came from"
        );
    }

    assert_eq!(after["installedCount"], 3);
    assert_eq!(after["missingCount"], 2, "the two external skills remain");
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
        let path = installed["installed"][0]["path"].as_str().unwrap();
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
        assert_eq!(first["installed"].as_array().unwrap().len(), 3);

        let second = call(client, "devup_skills", json!({"action": "install"})).await?;
        assert!(
            second["installed"].as_array().unwrap().is_empty(),
            "{second}"
        );
        assert_eq!(second["alreadyPresent"].as_array().unwrap().len(), 3);
        assert_eq!(second["state"]["installedCount"], 3);
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
        assert_eq!(known.len(), 5, "{known:?}");
        assert!(known.iter().any(|name| name == "devup-ui"));
        Ok(())
    })
    .await?;

    let _ = std::fs::remove_dir_all(&workspace);
    Ok(())
}
