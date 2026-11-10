#[cfg(test)]
mod retro_tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("devup-retro-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch directory");
        path
    }

    fn skill_text() -> &'static str {
        EMBEDDED
            .iter()
            .find(|(name, _)| *name == RETRO_NAME)
            .expect("the retro skill is embedded")
            .1[0]
            .1
    }

    /// A rename is one literal plus a directory move; every other spelling
    /// has to follow it or this fails.
    #[test]
    fn the_retro_name_is_spelled_in_one_place() {
        let record = &find_by_name(RETRO_NAME).expect("manifest entry").record;
        assert_eq!(record.origin, Origin::Own);
        assert!(record.path.ends_with(&format!("skills/{RETRO_NAME}")), "{}", record.path);
        assert!(record.source_url.ends_with(&format!("skills/{RETRO_NAME}")));
        assert!(record.title.starts_with(&format!("/{RETRO_NAME}:")), "{}", record.title);
        let text = skill_text();
        let frontmatter = &text[..frontmatter_end(text).expect("frontmatter")];
        assert!(frontmatter.contains(&format!("name: {RETRO_NAME}\n")), "{frontmatter}");
        assert!(frontmatter.contains(&format!("Invoke as /{RETRO_NAME} ")), "{frontmatter}");
        let command = Command::find(RETRO_NAME).expect("command");
        assert!(command.invocation.starts_with(&format!("/{RETRO_NAME} ")));
        assert_eq!(retro_prompt().name, RETRO_NAME);
    }

    /// The contract of the command, as the document an agent actually reads
    /// states it: review, then confirm, then prompts, and the rules that keep
    /// the list honest.
    #[test]
    fn the_skill_states_review_then_confirm_then_prompts() {
        let text = skill_text();
        let positions = ["## Phase 1: review", "## Phase 2: confirm", "## Phase 3: prompts"]
            .map(|heading| text.find(heading).unwrap_or_else(|| panic!("missing {heading}")));
        assert!(positions[0] < positions[1] && positions[1] < positions[2]);
        for required in [
            "Nothing is written to disk and no improvement prompt is printed before the user\nconfirms the review list.",
            "`action: \"retro\"`",
            "No invented problems. An item with no user-prompt evidence is excluded",
            "Not every problem is a devup-mcp bug.",
            "Do not strengthen the user's words.",
            "`quote`: the user's words, verbatim",
            "`OK` (Recommended) / `Edit items` / `Drop items` / `Cancel`",
            "Claude Code `AskUserQuestion`, OpenCode `question`",
            "never assume `OK`",
            "Only after `OK`.",
            "Korean when the session is\nKorean",
            "one ready-to-paste prompt per surface",
            "**Goal**",
            "**Required behaviour**",
            "**Acceptance checks and regression tests**",
            "**Scope boundary**",
            "`fixed-in-installed`",
            "`fixed-not-installed`",
            "`already-recorded`",
            "never guess \"fixed\"",
            "Without the folder, do not offer to create one.",
            "`feedback.nextNumber`",
            "Never renumber, rewrite or delete existing\nrecords",
        ] {
            assert!(text.contains(required), "the retro skill lost: {required}");
        }
        assert!(text.contains(&format!("**{}**", RETRO_PHASES.join(", "))));
    }

    /// The table in the skill and the list the tool returns are the same
    /// surfaces, so a classification made from one is valid in the other.
    #[test]
    fn the_skill_and_the_tool_name_the_same_surfaces() {
        let text = skill_text();
        let in_table = text
            .lines()
            .filter_map(|line| line.strip_prefix("| `")?.split_once('`').map(|(id, _)| id))
            .collect::<Vec<_>>();
        let in_tool = RETRO_SURFACES.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        assert_eq!(in_table, in_tool);
        for expected in ["codegen", "export-output", "validation-rule", "runtime-audit", "skill-guidance", "bootstrap-instructions", "project-plan-intake"] {
            assert!(in_tool.contains(&expected), "{expected}");
        }
        assert_eq!(in_tool.iter().filter(|id| id.starts_with("out-of-scope:")).count(), 3);
    }

    #[test]
    fn the_prompt_lists_optional_scope_and_language_and_carries_the_skill() {
        let prompt = retro_prompt();
        let arguments = prompt.arguments.as_deref().expect("arguments");
        assert_eq!(
            arguments.iter().map(|argument| argument.name.as_str()).collect::<Vec<_>>(),
            ["scope", "language"]
        );
        for argument in arguments {
            assert_eq!(argument.required, Some(false), "{}", argument.name);
            assert!(argument.description.as_deref().is_some_and(|text| !text.is_empty()));
        }
        assert!(prompt.description.as_deref().unwrap().starts_with(&format!("/{RETRO_NAME}:")));

        let arguments = serde_json::json!({"scope": "only the landing page", "language": " "});
        let text = retro_prompt_text(RETRO_NAME, arguments.as_object()).unwrap();
        assert!(text.starts_with(&format!("Run the {RETRO_NAME} command.\nscope: only the landing page\n\n# Session retrospective")), "{}", &text[..200]);
        assert!(!text.contains("
language:"), "a blank argument is not passed on");
        assert!(!text.contains("\n---\n"), "the frontmatter is the loader's, not the prompt's");
        assert!(text.contains("## Phase 2: confirm"));
        assert!(retro_prompt_text("devup", None).is_none());
        assert!(retro_prompt_result(RETRO_NAME, None).is_some());
    }

    #[test]
    fn the_call_log_orders_calls_and_keeps_names_not_contents() {
        let log = CallLog::default();
        let export = serde_json::json!({"action": "x", "outputs": ["tsx", 3], "frameIds": ["1:2", "3:4"], "projectRoot": "C:/secret/project"});
        log.begin("devup_figma_export", export.as_object()).finish(true);
        drop(log.begin("devup_ui_validate", None));
        let report = log.report();
        assert_eq!(report["served"], 2);
        assert_eq!(report["scope"], "this-server-process");
        let calls = report["calls"].as_array().unwrap();
        assert_eq!(calls[0]["seq"], 1);
        assert_eq!(calls[0]["tool"], "devup_figma_export");
        assert_eq!(calls[0]["ok"], true);
        assert_eq!(calls[0]["outputs"], serde_json::json!(["tsx"]));
        assert_eq!(calls[0]["target"], "frameIds=[\"1:2\",\"3:4\"]");
        assert_eq!(calls[1]["ok"], false, "a call that never finished is a failure");
        assert!(calls[1].get("action").is_none());
        assert!(!report.to_string().contains("secret"), "{report}");
        let at = calls[0]["at"].as_str().unwrap();
        assert!(at.len() == 20 && at.ends_with('Z') && at.as_bytes()[10] == b'T', "{at}");

        for _ in 0..CALL_LOG_CAPACITY + 5 {
            log.begin("devup_figma_measure", None).finish(true);
        }
        let report = log.report();
        assert_eq!(report["listed"], CALL_LOG_CAPACITY);
        assert_eq!(report["droppedOldest"], 7);
        assert_eq!(report["calls"][0]["seq"], 8);
    }

    #[test]
    fn timestamps_are_utc_calendar_dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_794_268_799_000), "2026-11-09T23:59:59Z");
        assert_eq!(rfc3339(1_798_761_600_000), "2027-01-01T00:00:00Z");
    }

    #[test]
    fn the_feedback_folder_is_read_for_numbering_and_never_created() {
        let project = scratch("feedback");
        let lookup = Lookup::project_only(project.clone(), Runtime::ClaudeCode);
        let log = CallLog::default();
        let report = retro_report(&lookup, &log);
        assert_eq!(report["feedback"]["exists"], false);
        assert!(!project.join(".feedback").exists(), "retro writes nothing");
        assert_eq!(report["phases"], serde_json::json!(["review", "confirm", "prompts"]));
        assert_eq!(report["surfaces"].as_array().unwrap().len(), RETRO_SURFACES.len());
        assert_eq!(report["command"]["name"], RETRO_NAME);
        assert_eq!(report["command"]["installed"], false);
        assert_eq!(report["skill"]["offlineRead"], format!("devup://skill/{RETRO_NAME}"));
        assert!(report["releases"]["installedTag"].as_str().unwrap().ends_with(env!("CARGO_PKG_VERSION")));

        let dir = project.join(".feedback");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("000-index.txt"), "Devup MCP 개선 피드백 목록\n").unwrap();
        std::fs::write(dir.join("001-first.txt"), "제목: 첫 항목\n분류: MCP\n상태: 수정됨\n").unwrap();
        std::fs::write(dir.join("016-korean-wrapping.txt"), "\n제목: 한글 줄바꿈\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a record").unwrap();
        let before = std::fs::read_dir(&dir).unwrap().count();
        let feedback = retro_report(&lookup, &log)["feedback"].clone();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), before);
        assert_eq!(feedback["exists"], true);
        assert_eq!(feedback["index"], "000-index.txt");
        assert_eq!(feedback["recordCount"], 2);
        assert_eq!(feedback["nextNumber"], 17);
        assert_eq!(feedback["nextFilePrefix"], "017-");
        assert_eq!(feedback["extension"], "txt");
        assert_eq!(
            feedback["readFirst"],
            serde_json::json!(["000-index.txt", "016-korean-wrapping.txt", "001-first.txt"])
        );
        assert_eq!(feedback["records"][0]["title"], "제목: 첫 항목");
        assert_eq!(feedback["records"][0]["status"], "상태: 수정됨");
        assert_eq!(feedback["records"][1]["title"], "제목: 한글 줄바꿈");
        assert!(feedback["records"][1]["status"].is_null());
        std::fs::remove_dir_all(project).unwrap();
    }

    /// /devup-retro per runtime, through the same install as /devup: Claude
    /// Code gets it as a skill, OpenCode as a command file generated from that
    /// skill, Codex is told there is no verified command location.
    #[tokio::test]
    async fn the_retro_command_is_installed_with_the_skills() {
        let project = scratch("command");
        let policy = super::super::output::OutputPolicy::from_roots(vec![project.clone()]).unwrap();
        let command = Command::find(RETRO_NAME).unwrap();

        let opencode = Lookup::project_only(project.clone(), Runtime::Opencode);
        let report = install(&policy, &opencode, &[RETRO_NAME.to_owned()]).await.unwrap();
        let written = report["commands"].as_array().unwrap();
        assert_eq!(
            written.iter().map(|entry| entry["name"].as_str().unwrap()).collect::<Vec<_>>(),
            [COMMAND_NAME, RETRO_NAME]
        );
        assert_eq!(written[1]["state"], "installed", "{report}");
        let file = project.join(".opencode").join("command").join(format!("{RETRO_NAME}.md"));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("---\ndescription: /"), "{}", &text[..80]);
        assert!(text.contains(&format!("<!-- devup-mcp:command {RETRO_NAME} - written by devup_skills install")));
        assert!(text.contains("$ARGUMENTS") && text.contains("## Phase 3: prompts"));
        assert_eq!(text.matches("\n---\n").count(), 1, "one frontmatter, the command's own");
        assert_eq!(command.state(&opencode)["installed"], true);
        let again = install(&policy, &opencode, &[RETRO_NAME.to_owned()]).await.unwrap();
        assert_eq!(again["commands"][1]["state"], "alreadyPresent");
        std::fs::write(&file, "mine").unwrap();
        let kept = install(&policy, &opencode, &[RETRO_NAME.to_owned()]).await.unwrap();
        assert_eq!(kept["commands"][1]["state"], "leftAlone");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");

        let claude = Lookup::project_only(project.clone(), Runtime::ClaudeCode);
        assert_eq!(command.state(&claude)["installed"], false);
        install(&policy, &claude, &[RETRO_NAME.to_owned()]).await.unwrap();
        let state = command.state(&claude);
        assert_eq!(state["installed"], true, "{state}");
        assert_eq!(state["perRuntime"][0]["via"], "skill");
        assert!(project.join(".claude").join("skills").join(RETRO_NAME).join("SKILL.md").is_file());
        assert!(state["how"].as_str().unwrap().contains(&format!("/{RETRO_NAME}")));

        let codex = command.state(&Lookup::project_only(project.clone(), Runtime::Codex));
        assert_eq!(codex["supported"], false);
        assert!(codex["how"].as_str().unwrap().contains("no command location"));
        assert!(codex["how"].as_str().unwrap().contains("codex"));
        drop(policy);
        std::fs::remove_dir_all(project).unwrap();
    }
}

#[cfg(test)]
mod retro_offer_tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("devup-retro-offer-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch directory");
        path
    }

    /// The offer is for a finished task. A result that does not mark
    /// completion gets nothing, so the sentence cannot ride on every reply.
    #[test]
    fn only_a_completion_result_carries_the_offer() {
        let project = scratch("gate");
        let lookup = Lookup::project_only(project.clone(), Runtime::ClaudeCode);
        let mut pending = serde_json::json!({"done": false});
        offer_retro(&mut pending, &lookup, false);
        assert!(pending.get(RETRO_OFFER_FIELD).is_none(), "{pending}");
        let mut not_an_object = serde_json::json!([]);
        offer_retro(&mut not_an_object, &lookup, true);
        assert_eq!(not_an_object, serde_json::json!([]));

        let mut done = serde_json::json!({"done": true});
        offer_retro(&mut done, &lookup, true);
        let offer = &done[RETRO_OFFER_FIELD];
        assert_eq!(offer["id"], RETRO_NAME);
        assert_eq!(offer["when"], "final-completion-report");
        // Not installed here: the MCP prompt form is the one that works.
        assert_eq!(offer["skillInstalled"], false);
        assert_eq!(offer["invocation"], format!("/mcp__devup-mcp__{RETRO_NAME}"));
        assert_eq!(
            offer["message"]["ko"],
            format!("작업이 완료되었습니다. 지금까지 진행한 내용을 개선하기 위해 /mcp__devup-mcp__{RETRO_NAME} 를 실행할까요?")
        );
        let how = offer["how"].as_str().unwrap();
        for rule in ["once per completed task", "not after an intermediate call, a failure", "Never run it yourself", "user's language"] {
            assert!(how.split_whitespace().collect::<Vec<_>>().join(" ").contains(rule), "{rule}");
        }
        std::fs::remove_dir_all(project).unwrap();
    }

    #[tokio::test]
    async fn an_installed_skill_is_offered_as_the_slash_command() {
        let project = scratch("installed");
        let policy = super::super::output::OutputPolicy::from_roots(vec![project.clone()]).unwrap();
        let lookup = Lookup::project_only(project.clone(), Runtime::ClaudeCode);
        install(&policy, &lookup, &[RETRO_NAME.to_owned()]).await.unwrap();
        let mut done = serde_json::json!({});
        offer_retro(&mut done, &lookup, true);
        let offer = &done[RETRO_OFFER_FIELD];
        assert_eq!(offer["skillInstalled"], true);
        assert_eq!(offer["invocation"], format!("/{RETRO_NAME}"));
        assert!(offer["message"]["ko"].as_str().unwrap().contains(&format!("/{RETRO_NAME} 를 실행할까요?")));
        assert!(offer["message"]["en"].as_str().unwrap().contains(&format!("Shall I run /{RETRO_NAME} ")));
        drop(policy);
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn only_three_results_mark_completion() {
        use serde_json::json;
        let receipt = |allowed: bool| json!({"kind": "devup-completion-receipt", "stage": if allowed {"data-verified"} else {"browser-verified"}, "done": allowed, "reportAllowed": allowed});
        assert!(marks_completion("devup_runtime_audit", "completion", &receipt(true)));
        assert!(!marks_completion("devup_runtime_audit", "completion", &receipt(false)));
        assert!(!marks_completion("devup_runtime_audit", "plan", &receipt(true)));
        assert!(!marks_completion("devup_runtime_audit", "ingest", &json!({"verdict": "pass"})));

        let status = |allowed: bool| json!({"done": allowed, "completion": {"done": allowed, "reportAllowed": allowed}});
        assert!(marks_completion("devup_screen_plan", "status", &status(true)));
        assert!(!marks_completion("devup_screen_plan", "status", &status(false)));
        assert!(!marks_completion("devup_screen_plan", "auditPlan", &status(true)));

        let layer = |checked: bool, drifts: serde_json::Value| json!({"checked": checked, "drifts": drifts});
        let stack = |layers: serde_json::Value| json!({"found": true, "layers": layers});
        let clean = || layer(true, json!([]));
        let all = stack(json!({"db-entity": clean(), "entity-route": clean(), "route-openapi": clean(), "openapi-client": clean()}));
        assert!(marks_completion("devup_stack_diff", "", &all));
        let drifted = stack(json!({"db-entity": clean(), "entity-route": layer(true, json!([{"kind": "missing"}])), "route-openapi": clean(), "openapi-client": clean()}));
        assert!(!marks_completion("devup_stack_diff", "", &drifted));
        let skipped = stack(json!({"db-entity": layer(false, json!([])), "entity-route": clean(), "route-openapi": clean(), "openapi-client": clean()}));
        assert!(!marks_completion("devup_stack_diff", "", &skipped), "an unchecked layer is not zero drift");
        let partial = stack(json!({"db-entity": clean()}));
        assert!(!marks_completion("devup_stack_diff", "", &partial), "one layer is not the stack");
        assert!(!marks_completion("devup_stack_diff", "", &json!({"found": false})));

        // The export never marks completion, whatever its deliverable says,
        // and neither does a clean validation: both happen many times a task.
        let export = json!({"deliverable": {"isFinal": true}, "reportAllowed": true, "completion": {"reportAllowed": true}});
        assert!(!marks_completion("devup_figma_export", "", &export));
        assert!(!marks_completion("devup_ui_validate", "", &json!({"errors": [], "completion": {"reportAllowed": true}})));
    }

    /// Wherever an agent reads how to finish, it reads the same question.
    #[test]
    fn every_completion_text_states_the_offer_and_its_limits() {
        let ko = RETRO_OFFER_KO.replace("{}", &format!("/{RETRO_NAME}"));
        let en = RETRO_OFFER_EN.replace("{}", &format!("/{RETRO_NAME}"));
        let flat = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
        let embedded = |name: &str, path: &str| {
            EMBEDDED.iter().find(|(key, _)| *key == name).unwrap().1.iter().find(|(file, _)| *file == path).unwrap().1
        };
        let guide = super::super::guide::GUIDE;
        for (label, text) in [
            ("delivery-quality", embedded("devfive-frontend", "references/delivery-quality.md")),
            ("devfive-frontend", embedded("devfive-frontend", "SKILL.md")),
            ("devup", embedded("devup", "SKILL.md")),
            ("devup-implementation-plan", embedded("devup-implementation-plan", "SKILL.md")),
            ("guide", guide),
        ] {
            let text = flat(text);
            assert!(text.contains(&ko), "{label}: Korean offer");
            assert!(text.contains(&flat(&en)), "{label}: English offer");
            assert!(text.contains(&format!("/mcp__<server>__{RETRO_NAME}")), "{label}: prompt form");
            assert!(text.to_lowercase().contains("once"), "{label}: once");
            assert!(text.to_lowercase().contains("never run it") || text.contains("never run unasked"), "{label}: never auto-run");
        }
        let opencode = flat(COMMANDS[0].opencode.unwrap());
        assert!(opencode.contains(&ko) && opencode.contains("never run it unasked"));
        assert!(guide.contains("`reportAllowed: true` carries `nextSuggestedAction`"));
        assert!(!super::super::guide::INSTRUCTIONS.contains(RETRO_NAME), "the bootstrap budget is spent elsewhere");
    }
}
