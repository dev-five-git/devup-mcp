//! The export has to name the conventions its own output is written in.
//!
//! `devup_ui_validate` already did, but only for a caller who had written the
//! code and then chosen to validate it - and an agent that has never seen
//! devup-ui does neither. These lock the earlier moment.
//!
//! Its own file rather than a module in `mod.rs` because these tests touch the
//! filesystem, and `tests/module_boundaries.rs` holds the router to having no
//! `std::fs::` in it at all.

use super::*;
fn scratch(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "devup-export-gap-{label}-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// The whole point: the gap arrives on the response that carries the code,
/// without the caller having asked anything of it.
#[test]
fn an_export_that_returns_tsx_names_the_skills_it_is_written_in() {
    let project = scratch("tsx");
    let checked = with_project_checks(
        json!({"status": "complete", "tsx": "<Box />"}),
        project.to_str(),
        &skills::Lookup::project_only(project.clone(), skills::Runtime::Codex),
    );
    let gap = &checked["skillGap"];
    assert!(!gap.is_null(), "an export carrying TSX said nothing");
    assert_eq!(gap["runtime"], "codex");
    let named = gap["missing"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["skill"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(named.contains(&"devup-ui".to_owned()));
    assert!(named.contains(&"devfive-frontend".to_owned()));
    // The one action that closes it, not advice.
    assert_eq!(gap["missing"][0]["install"]["action"], "devup_skills");

    let _ = std::fs::remove_dir_all(&project);
}

/// Every Section workflow answers under `frames` and nothing at the top
/// level. Reading only the top level is how the theme check used to miss
/// them, and this must not repeat it.
#[test]
fn a_section_export_is_checked_through_its_frames() {
    let project = scratch("frames");
    let checked = with_project_checks(
        json!({"status": "complete", "frames": [{"nodeId": "1:2", "tsx": "<Box />"}]}),
        project.to_str(),
        &skills::Lookup::project_only(project.clone(), skills::Runtime::Codex),
    );
    assert!(!checked["skillGap"].is_null(), "frames were not read");

    let _ = std::fs::remove_dir_all(&project);
}

/// The caller who does not know about devup-ui is exactly the one who
/// sends no `projectRoot`. Gating the gap on it would have hidden it from
/// everyone who needed it.
#[test]
fn the_gap_does_not_need_a_project_root() {
    let project = scratch("no-root");
    let checked = with_project_checks(
        json!({"status": "complete", "tsx": "<Box />"}),
        None,
        &skills::Lookup::project_only(project.clone(), skills::Runtime::Codex),
    );
    assert!(!checked["skillGap"].is_null());
    assert_eq!(
        checked["skillGap"]["workspace"],
        project.display().to_string()
    );

    let _ = std::fs::remove_dir_all(&project);
}

/// Telling someone to install what they already have is the noise that
/// teaches them to ignore the field.
#[test]
fn nothing_is_said_when_the_skills_are_already_there() {
    let project = scratch("present");
    for name in CODE_SKILLS {
        let path = skills::install_path(&project.join(".agents").join("skills"), name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# already here").unwrap();
    }
    let checked = with_project_checks(
        json!({"status": "complete", "tsx": "<Box />"}),
        project.to_str(),
        &skills::Lookup::project_only(project.clone(), skills::Runtime::Codex),
    );
    assert!(
        checked["skillGap"].is_null(),
        "installed skills were reported as a gap"
    );

    let _ = std::fs::remove_dir_all(&project);
}

/// An outdated skill is the case `installed: true` used to hide: it loads, it
/// is read, and it teaches rules this build no longer emits. It has to be said
/// apart from missing, because the two need different words.
#[test]
fn an_outdated_skill_is_reported_apart_from_a_missing_one() {
    let project = scratch("outdated");
    let root = project.join(".agents").join("skills");
    // devup-ui installed as this build writes it; devfive-frontend from an
    // older one, recognisable by its provenance note and nothing else.
    for name in CODE_SKILLS {
        let skill = skills::find_by_name(name).unwrap();
        for (relative, contents) in skill.installable_documents().unwrap() {
            let path = relative
                .split('/')
                .fold(root.join(name), |path, part| path.join(part));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
        }
    }
    let entry = skills::install_path(&root, "devfive-frontend");
    let body = std::fs::read_to_string(&entry).unwrap();
    std::fs::write(&entry, body.replace("Server Components", "an older build")).unwrap();

    let checked = with_project_checks(
        json!({"status": "complete", "tsx": "<Box />"}),
        project.to_str(),
        &skills::Lookup::project_only(project.clone(), skills::Runtime::Codex),
    );
    let gap = &checked["skillGap"];
    assert!(!gap.is_null(), "a stale skill read as fine");
    assert!(
        gap["missing"].as_array().unwrap().is_empty(),
        "nothing is missing here: {gap}"
    );
    assert_eq!(gap["outdated"][0]["skill"], "devfive-frontend");
    assert_eq!(gap["outdated"][0]["revision"], "older");
    assert_eq!(gap["outdated"][0]["install"]["action"], "devup_skills");

    let _ = std::fs::remove_dir_all(&project);
}

/// A response with no code in it has nothing to be written in.
#[test]
fn a_selection_response_carries_no_gap() {
    let project = scratch("selection");
    let checked = with_project_checks(
        json!({"status": "selection_required", "selection": {"count": 3}}),
        project.to_str(),
        &skills::Lookup::project_only(project.clone(), skills::Runtime::Codex),
    );
    assert!(checked["skillGap"].is_null());

    let _ = std::fs::remove_dir_all(&project);
}

/// The export gap covers the code devup-mcp writes; this covers the files it
/// reads. Scoped, because a project with no openapi.json has no use for the
/// vespera skill and naming it there is the noise that teaches a reader to
/// skip the field.
#[test]
fn project_context_names_only_the_skills_for_what_that_scope_found() {
    let project = scratch("context");
    let lookup = skills::Lookup::project_only(project.clone(), skills::Runtime::Codex);

    let api = with_context_skill_gap(json!({"found": true, "scope": "api"}), "api", &lookup);
    let named = |gap: &Value| {
        gap["missing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["skill"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(named(&api["skillGap"]), vec!["vespera"]);

    let db = with_context_skill_gap(json!({"found": true, "scope": "db"}), "db", &lookup);
    assert_eq!(named(&db["skillGap"]), vec!["vespertide"]);

    // Nothing found: nothing to say. vespera is irrelevant to a project with
    // no openapi.json.
    let empty = with_context_skill_gap(json!({"found": false}), "api", &lookup);
    assert!(empty["skillGap"].is_null(), "{empty}");

    // `all` nests one object per scope and each one answers for itself.
    let all = with_context_skill_gap(
        json!({"found": true, "theme": {"found": true}, "api": {"found": false}, "db": {"found": true}}),
        "all",
        &lookup,
    );
    assert_eq!(named(&all["skillGap"]), vec!["devup-ui", "vespertide"]);

    let _ = std::fs::remove_dir_all(&project);
}

/// The tool description is the only channel that reaches a subagent handed
/// tool schemas alone, or Codex, where server `instructions` becomes one
/// namespace blurb rather than prompt text. If the sentence is dropped
/// from there, the nudge is gone for both.
#[test]
fn the_export_tool_description_still_says_to_call_devup_skills() {
    let tool = DevupServer::tool_router()
        .list_all()
        .into_iter()
        .find(|tool| tool.name == "devup_figma_export")
        .expect("devup_figma_export is registered");
    let description = tool.description.clone().unwrap_or_default();
    assert!(
        description.contains("devup_skills"),
        "the export description no longer names the tool that closes the gap"
    );
    assert!(
        description.contains("devup-ui"),
        "the export description no longer says what the TSX is written in"
    );
    // The three guide rules whose violation produces wrong code rather than a
    // worse response. They lived only in a resource that Codex shows as a
    // single namespace blurb, which is to say nowhere the model reads them.
    for rule in ["screenshot", "Never guess", "node tree"] {
        assert!(
            description.contains(rule),
            "the rule about {rule} is back to living only in the guide"
        );
    }
}
