//! The retrospective command, through the real process.
//!
//! The unit tests hold the skill document and the report to each other. These
//! hold what a client actually sees: a prompt in `prompts/list` it can offer as
//! a command without any install, and a `devup_skills` action that reports the
//! calls this process served and the feedback folder without writing anything.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ContentBlock, GetPromptRequestParams},
};
use serde_json::{Map, Value, json};

/// The command's name, spelled once on this side of the process boundary.
const NAME: &str = "devup-retro";

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("devup-retro-it-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch workspace");
    // The server canonicalises its write root; on macOS the temp directory is
    // a symlink, and a projectRoot spelled through it is outside that root.
    dunce::canonicalize(&path).expect("canonical scratch workspace")
}

async fn session<F, T>(workspace: &Path, body: F) -> anyhow::Result<T>
where
    F: AsyncFnOnce(&rmcp::service::RunningService<rmcp::RoleClient, ()>) -> anyhow::Result<T>,
{
    // An empty home, so a skill installed on the machine running the tests is
    // not counted as installed here.
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
    arguments: Value,
) -> anyhow::Result<Value> {
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("devup_skills").with_arguments(arguments))
        .await?;
    Ok(result.structured_content.expect("a structured response"))
}

#[tokio::test]
async fn the_prompt_is_listed_with_optional_arguments_and_returns_the_three_phases()
-> anyhow::Result<()> {
    let workspace = scratch("prompt");
    session(&workspace, async |client| {
        let capabilities = &client.peer_info().expect("server info").capabilities;
        assert!(
            capabilities.prompts.is_some(),
            "prompts capability is advertised"
        );

        let prompts = client.list_all_prompts().await?;
        assert_eq!(prompts.len(), 1, "{prompts:?}");
        let prompt = &prompts[0];
        assert_eq!(prompt.name, NAME);
        let arguments = prompt.arguments.as_deref().expect("arguments");
        assert_eq!(
            arguments
                .iter()
                .map(|argument| argument.name.as_str())
                .collect::<Vec<_>>(),
            ["scope", "language"]
        );
        assert!(
            arguments
                .iter()
                .all(|argument| argument.required == Some(false))
        );

        let given: Map<String, Value> =
            json!({"scope": "since the second export", "language": "ko"})
                .as_object()
                .cloned()
                .unwrap();
        let result = client
            .get_prompt(GetPromptRequestParams::new(NAME).with_arguments(given))
            .await?;
        assert_eq!(result.messages.len(), 1);
        let ContentBlock::Text(text) = &result.messages[0].content else {
            panic!("the prompt is one text message: {:?}", result.messages[0]);
        };
        let text = &text.text;
        assert!(
            text.contains("scope: since the second export\nlanguage: ko\n"),
            "{}",
            &text[..200]
        );
        let phases = [
            "## Phase 1: review",
            "## Phase 2: confirm",
            "## Phase 3: prompts",
        ]
        .map(|heading| {
            text.find(heading)
                .unwrap_or_else(|| panic!("missing {heading}"))
        });
        assert!(phases[0] < phases[1] && phases[1] < phases[2]);

        assert!(
            client
                .get_prompt(GetPromptRequestParams::new("nope"))
                .await
                .is_err(),
            "an unknown prompt is an error, not an empty prompt"
        );
        Ok(())
    })
    .await?;
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn retro_reports_the_served_calls_and_the_feedback_folder_and_writes_nothing()
-> anyhow::Result<()> {
    let workspace = scratch("action");
    let project = workspace.join("project");
    let feedback = project.join(".feedback");
    std::fs::create_dir_all(&feedback)?;
    std::fs::write(feedback.join("000-index.txt"), "index\n")?;
    std::fs::write(
        feedback.join("033-axe-core.txt"),
        "제목: axe-core\n상태: open\n",
    )?;
    let root = project.display().to_string();

    session(&workspace, async |client| {
        let status = call(client, json!({"action": "status", "projectRoot": root})).await?;
        assert!(
            status["skills"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["name"] == NAME),
            "the skill is discoverable through devup_skills status"
        );
        // A refused call is served too, and logged as not ok.
        let refused = client
            .call_tool(
                CallToolRequestParams::new("devup_skills").with_arguments(
                    json!({"action": "nope", "projectRoot": root})
                        .as_object()
                        .cloned()
                        .unwrap(),
                ),
            )
            .await?;
        assert_eq!(refused.is_error, Some(true));

        let retro = call(client, json!({"action": "retro", "projectRoot": root})).await?;
        assert!(retro["server"]["version"].is_string(), "{retro}");
        assert!(retro["server"]["buildId"].is_string());
        assert_eq!(retro["phases"], json!(["review", "confirm", "prompts"]));
        assert_eq!(retro["command"]["name"], NAME);
        assert_eq!(retro["command"]["installed"], false);
        assert_eq!(
            retro["skill"]["offlineRead"],
            format!("devup://skill/{NAME}")
        );
        assert!(
            retro["surfaces"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["id"] == "validation-rule")
        );

        let calls = retro["callLog"]["calls"].as_array().unwrap();
        assert_eq!(
            calls.len(),
            2,
            "the retro call itself is logged when it returns: {retro}"
        );
        assert_eq!(calls[0]["tool"], "devup_skills");
        assert_eq!(calls[0]["action"], "status");
        assert_eq!(calls[0]["ok"], true);
        assert_eq!(calls[1]["action"], "nope");
        assert_eq!(calls[1]["ok"], false);
        assert!(calls[0]["at"].as_str().unwrap().ends_with('Z'));

        assert_eq!(retro["feedback"]["exists"], true);
        assert_eq!(retro["feedback"]["index"], "000-index.txt");
        assert_eq!(retro["feedback"]["nextNumber"], 34);
        assert_eq!(retro["feedback"]["nextFilePrefix"], "034-");
        assert_eq!(retro["feedback"]["records"][0]["status"], "상태: open");

        // One install makes it a command, next to /devup.
        let installed = call(
            client,
            json!({"action": "install", "names": [NAME], "projectRoot": root}),
        )
        .await?;
        assert_eq!(installed["installed"][0]["name"], NAME, "{installed}");
        let after = call(client, json!({"action": "retro", "projectRoot": root})).await?;
        assert_eq!(after["command"]["installed"], true, "{after}");
        assert_eq!(after["skill"]["installed"], true);
        Ok(())
    })
    .await?;

    let mut names = std::fs::read_dir(&feedback)?
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        ["000-index.txt", "033-axe-core.txt"],
        "retro never writes feedback"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

/// The offer rides only on a result that marks completion. Through the real
/// process, a stack diff that found nothing to compare, a skills status and a
/// retro report are not that, and must not carry it.
#[tokio::test]
async fn results_that_do_not_mark_completion_carry_no_offer() -> anyhow::Result<()> {
    let workspace = scratch("no-offer");
    let root = workspace.display().to_string();
    session(&workspace, async |client| {
        let arguments: Map<String, Value> =
            json!({"projectRoot": root}).as_object().cloned().unwrap();
        let diff = client
            .call_tool(CallToolRequestParams::new("devup_stack_diff").with_arguments(arguments))
            .await?
            .structured_content
            .expect("a structured response");
        assert!(diff.get("nextSuggestedAction").is_none(), "{diff}");
        for action in ["status", "retro"] {
            let result = call(client, json!({"action": action, "projectRoot": root})).await?;
            assert!(result.get("nextSuggestedAction").is_none(), "{action}");
        }
        Ok(())
    })
    .await?;
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}
