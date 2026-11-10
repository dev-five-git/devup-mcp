/// What the retrospective command needs from the server.
///
/// The command itself is a skill: reading a conversation and judging which of
/// the user's messages were corrections is the agent's work, and nothing here
/// tries to do it. What the agent cannot produce on its own is what this
/// process knows - which build it is, which devup-mcp calls it served and
/// when, and what the project's `.feedback/` folder already records - so
/// `devup_skills` action `retro` reports exactly that and writes nothing.
const CALL_LOG_CAPACITY: usize = 200;
const RELEASES_URL: &str = "https://github.com/dev-five-git/devup-mcp/releases";
const FEEDBACK_DIR: &str = ".feedback";
const FEEDBACK_LISTED: usize = 200;

/// The surfaces a follow-up prompt is classified under. The skill document
/// carries the same list as a table; a test holds the two together.
pub const RETRO_SURFACES: &[(&str, &str)] = &[
    ("codegen", "The TSX/CSS the converter generates from Figma nodes."),
    ("export-output", "What devup_figma_export returns: outputs, assets, manifests, deliverable flags, sidecars."),
    ("validation-rule", "A devup_ui_validate rule that is missing, too weak or wrong."),
    ("runtime-audit", "devup_runtime_audit, audit plans, devup_visual_compare checks and their evidence."),
    ("skill-guidance", "The installed skills: devup, devup-implementation-plan, devfive-frontend and references."),
    ("bootstrap-instructions", "The server instructions and devup://guide/usage."),
    ("project-plan-intake", "devup_project_plan / devup_screen_plan questions, rounds, ERD, split."),
    ("out-of-scope:devup-ui", "Devup UI runtime or compiler, not devup-mcp."),
    ("out-of-scope:app", "The application's own code, tests, CI or environment."),
    ("out-of-scope:figma-data", "The design file itself."),
];

/// The order the command runs in. Nothing is written, and no improvement
/// prompt is printed, before `confirm`.
pub const RETRO_PHASES: [&str; 3] = ["review", "confirm", "prompts"];

#[derive(Debug, Clone)]
struct CallEntry {
    seq: u64,
    tool: String,
    action: Option<String>,
    outputs: Vec<String>,
    target: Option<String>,
    at_ms: u64,
    duration_ms: u64,
    ok: bool,
}

#[derive(Debug, Default)]
struct CallLogState {
    served: u64,
    entries: std::collections::VecDeque<CallEntry>,
}

/// The devup-mcp calls this server process served, newest last, bounded.
///
/// It exists so a retrospective can line a user's correction up with the
/// export or validation it followed. It holds names and times, never
/// arguments' contents or results, and it dies with the process: a reconnect
/// starts an empty log, and the report says so instead of implying a session
/// with no calls.
#[derive(Debug, Clone, Default)]
pub struct CallLog(std::sync::Arc<std::sync::Mutex<CallLogState>>);

/// Records its call when dropped, so every return path of `call_tool` is
/// covered. A path that never reached [`CallRecord::finish`] is a failure.
pub struct CallRecord {
    log: CallLog,
    entry: CallEntry,
    started: std::time::Instant,
}

impl CallLog {
    pub fn begin(
        &self,
        tool: &str,
        arguments: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> CallRecord {
        let text = |key: &str| {
            arguments
                .and_then(|arguments| arguments.get(key))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let target = ["frameIds", "nodeIds", "nodeId", "url", "baseUrl", "planId"]
            .iter()
            .find_map(|key| {
                let value = arguments?.get(*key).filter(|value| !value.is_null())?;
                let rendered = value.as_str().map_or_else(|| value.to_string(), str::to_owned);
                Some(format!("{key}={}", rendered.chars().take(120).collect::<String>()))
            });
        CallRecord {
            log: self.clone(),
            entry: CallEntry {
                seq: 0,
                tool: tool.to_owned(),
                action: text("action"),
                outputs: arguments
                    .and_then(|arguments| arguments.get("outputs"))
                    .and_then(serde_json::Value::as_array)
                    .map(|outputs| {
                        outputs
                            .iter()
                            .filter_map(|output| output.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                target,
                at_ms: now_ms(),
                duration_ms: 0,
                ok: false,
            },
            started: std::time::Instant::now(),
        }
    }

    pub fn report(&self) -> serde_json::Value {
        let state = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let calls = state
            .entries
            .iter()
            .map(|entry| {
                let mut call = serde_json::json!({
                    "seq": entry.seq,
                    "tool": entry.tool,
                    "at": rfc3339(entry.at_ms),
                    "durationMs": entry.duration_ms,
                    "ok": entry.ok,
                });
                if let Some(action) = &entry.action {
                    call["action"] = serde_json::json!(action);
                }
                if !entry.outputs.is_empty() {
                    call["outputs"] = serde_json::json!(entry.outputs);
                }
                if let Some(target) = &entry.target {
                    call["target"] = serde_json::json!(target);
                }
                call
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "scope": "this-server-process",
            "served": state.served,
            "listed": calls.len(),
            "droppedOldest": state.served - calls.len() as u64,
            "calls": calls,
            "note": "Calls this devup-mcp process served, oldest first, UTC. A reconnect, a \
                     restart or a resumed session starts an empty log, so a short log is not \
                     evidence that nothing was called: the conversation is the record, and this \
                     only helps order it. Arguments' contents and results are not kept.",
        })
    }
}

impl CallRecord {
    pub fn finish(&mut self, ok: bool) {
        self.entry.ok = ok;
    }
}

impl Drop for CallRecord {
    fn drop(&mut self) {
        let mut entry = self.entry.clone();
        entry.duration_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let mut state = self.log.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.served += 1;
        entry.seq = state.served;
        if state.entries.len() == CALL_LOG_CAPACITY {
            state.entries.pop_front();
        }
        state.entries.push_back(entry);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

/// Unix milliseconds as `YYYY-MM-DDTHH:MM:SSZ`, by the civil-from-days
/// algorithm, so a timestamp does not cost a date crate.
fn rfc3339(ms: u64) -> String {
    let seconds = ms / 1000;
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// What the project's `.feedback/` folder already holds, so new records
/// continue its numbering and nothing already recorded is filed twice.
///
/// Read-only. A record is a file named `<number>-<slug>.<ext>`; number zero
/// is the index. The format inside is the project's own, so this reports the
/// files to read rather than a schema to follow.
fn feedback_state(project: &Path) -> serde_json::Value {
    let dir = project.join(FEEDBACK_DIR);
    let Ok(listing) = std::fs::read_dir(&dir) else {
        return serde_json::json!({
            "exists": false,
            "path": dir.display().to_string(),
            "how": "No .feedback folder in this project: do not offer to save records and do \
                    not create the folder.",
        });
    };
    let mut records = listing
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let (number, _) = name.split_once('-')?;
            let digits = number.len();
            Some((number.parse::<u32>().ok()?, digits, name))
        })
        .collect::<Vec<_>>();
    records.sort();
    let index = records
        .iter()
        .find(|(number, ..)| *number == 0)
        .map(|(.., name)| name.clone());
    records.retain(|(number, ..)| *number > 0);
    let last = records.last();
    let next = last.map_or(1, |(number, ..)| number + 1);
    let digits = last.map_or(3, |(_, digits, _)| *digits);
    let skipped = records.len().saturating_sub(FEEDBACK_LISTED);
    let listed = records
        .iter()
        .skip(skipped)
        .map(|(number, _, name)| {
            let text = std::fs::read_to_string(dir.join(name)).unwrap_or_default();
            let lines = || text.lines().take(12).map(str::trim).filter(|line| !line.is_empty());
            let clip = |line: &str| line.chars().take(200).collect::<String>();
            serde_json::json!({
                "number": number,
                "file": name,
                "title": lines().next().map(clip),
                "status": lines()
                    .find(|line| ["상태:", "Status:", "status:"].iter().any(|label| line.starts_with(label)))
                    .map(clip),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "exists": true,
        "path": dir.display().to_string(),
        "index": index,
        "recordCount": records.len(),
        "nextNumber": next,
        "nextFilePrefix": format!("{next:0digits$}-"),
        "extension": last.and_then(|(.., name)| name.rsplit_once('.')).map(|(_, extension)| extension),
        "readFirst": index.iter().cloned().chain(records.iter().rev().take(2).map(|(.., name)| name.clone())).collect::<Vec<_>>(),
        "records": listed,
        "recordsOmitted": skipped,
        "how": "Read the index and the two most recent records before writing, and follow their \
                format, language and file naming exactly. Write only after the user confirmed \
                the review list and chose to save. Never renumber or rewrite existing records.",
    })
}

/// The `devup_skills` action `retro` result. `server` is added by the tool
/// result wrapper, like on every other response.
pub fn retro_report(lookup: &Lookup, calls: &CallLog) -> serde_json::Value {
    let command = Command::find(RETRO_NAME).expect("the retro command is registered");
    serde_json::json!({
        "command": command.state(lookup),
        "skill": {
            "name": RETRO_NAME,
            "installed": !lookup.installed_paths(RETRO_NAME).is_empty(),
            "offlineRead": uri_for(RETRO_NAME),
            "prompt": RETRO_NAME,
        },
        "phases": RETRO_PHASES,
        "surfaces": RETRO_SURFACES
            .iter()
            .map(|(id, covers)| serde_json::json!({"id": id, "covers": covers}))
            .collect::<Vec<_>>(),
        "callLog": calls.report(),
        "feedback": feedback_state(&lookup.project),
        "releases": {
            "url": RELEASES_URL,
            "installedTag": format!("devup-mcp(crates/devup-mcp/Cargo.toml)@{}", env!("CARGO_PKG_VERSION")),
            "how": "An item is fixed-in-installed only when release notes at or before the \
                    installed version (server.version, server.commit) name the fix, and \
                    fixed-not-installed only when a later release does (server.updateAvailable). \
                    When the notes cannot be read the status is unverified, never a guess.",
        },
        "next": format!(
            "Follow the {RETRO_NAME} skill: review the conversation from the first devup-mcp \
             call, list only follow-ups the user actually wrote, ask the user to confirm the \
             list, and only then print the improvement prompts. Nothing is written before \
             confirmation."
        ),
        "boundary": "This action reads and reports; it never writes. It does not read the \
                     conversation and cannot tell which user messages were corrections.",
    })
}

/// The MCP prompt of the same name: the skill without an install. A client
/// that lists prompts offers it as a command (Claude Code shows
/// `/mcp__<server>__<name>`), which is the only way to reach the command in a
/// workspace where `devup_skills install` was never run.
pub const RETRO_PROMPT_ARGUMENTS: [(&str, &str); 2] = [
    ("scope", "Which part of the session to review, for example \"only the landing page\" or \"since the second export\". Default: everything from the first devup-mcp call."),
    ("language", "Language for the review list and the improvement prompts. Default: the language the user wrote in."),
];

pub fn retro_prompt() -> rmcp::model::Prompt {
    let title = find_by_name(RETRO_NAME).map_or(RETRO_NAME, |skill| &skill.record.title);
    rmcp::model::Prompt::new(
        RETRO_NAME,
        Some(title),
        Some(
            RETRO_PROMPT_ARGUMENTS
                .iter()
                .map(|(name, description)| {
                    rmcp::model::PromptArgument::new(*name)
                        .with_description(*description)
                        .with_required(false)
                })
                .collect(),
        ),
    )
}

/// The prompt's one user message: the arguments that were given, then the
/// skill body. `None` for a name that is not this prompt's.
pub fn retro_prompt_text(
    name: &str,
    arguments: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<String> {
    if name != RETRO_NAME {
        return None;
    }
    let mut text = format!("Run the {RETRO_NAME} command.\n");
    for (key, _) in RETRO_PROMPT_ARGUMENTS {
        if let Some(value) = arguments
            .and_then(|arguments| arguments.get(key))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            text.push_str(&format!("{key}: {value}\n"));
        }
    }
    text.push('\n');
    text.push_str(skill_body(RETRO_NAME).trim_start());
    Some(text)
}

pub fn retro_prompt_result(
    name: &str,
    arguments: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<rmcp::model::GetPromptResponse> {
    let message = rmcp::model::PromptMessage::new_text(
        rmcp::model::Role::User,
        retro_prompt_text(name, arguments)?,
    );
    let title = find_by_name(RETRO_NAME).map_or(RETRO_NAME, |skill| &skill.record.title);
    Some(rmcp::model::GetPromptResult::new(vec![message]).with_description(title).into())
}
