//! Single frames against what the plugin wrote for them.
//!
//! `fixtures/plugin-answers/<name>/pure.tsx` is the plugin's Pure Code for one
//! frame of `devup-Test`, and `fixtures/local-screens/<name>.json` is the
//! capture of that frame. The captures are not committed, so each comparison
//! skips when its capture is absent rather than pretending to have checked.
//!
//! Every line of the answer has to be written here, and every line written
//! here has to be in the answer, except the ones each frame names as
//! different on purpose, with the reason.

use std::{fs, path::PathBuf};

use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    ui_validate::{Severity, validate_devup_ui_tsx},
};
use devup_mcp_figma::{Snapshot, UpstreamResult};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn ours(capture: &str) -> Option<String> {
    ours_of(capture, None)
}

/// The Pure Code of one root of a capture — the first, or the one named.
fn ours_of(capture: &str, root: Option<&str>) -> Option<String> {
    let raw = fs::read_to_string(fixtures().join(format!("local-screens/{capture}.json"))).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let snapshot: Snapshot = serde_json::from_value(value.get("snapshot")?.clone()).ok()?;
    let resource = |name: &str| {
        value
            .get("payload")
            .and_then(|payload| payload.get(name))
            .cloned()
            .map(|raw| UpstreamResult { raw })
    };
    let variables = resource("variables");
    let styles = resource("styles");
    // The answer is the plugin's Pure Code, with every instance expanded to
    // primitives.
    let options = CodegenOptions {
        inline_instances: true,
        ..CodegenOptions::default()
    }
    .with_resource_results(variables.as_ref(), styles.as_ref());
    let root = match root {
        Some(root) => root.to_owned(),
        None => snapshot.roots.first()?.clone(),
    };
    Some(generate_component(&snapshot, &root, &options).ok()?.tsx)
}

/// A file's lines as they are compared: indentation and blank lines dropped.
fn comparable_lines(source: &str) -> Vec<String> {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The lines a module has around the JSX the answer is: imports, the
/// function, and its return.
fn module_wrapper(line: &str) -> bool {
    line.starts_with("import ")
        || line.starts_with("export function ")
        || line == "return ("
        || line == ");"
        || line == "}"
}

fn matches_the_answer(name: &str, differs_on_purpose: &[(&str, &str)]) {
    let Some(tsx) = ours(name) else {
        eprintln!("no {name} capture; skipping");
        return;
    };
    compare(name, &tsx, differs_on_purpose);
}

fn compare(name: &str, tsx: &str, differs_on_purpose: &[(&str, &str)]) {
    let Ok(answer) = fs::read_to_string(fixtures().join(format!("plugin-answers/{name}/pure.tsx")))
    else {
        eprintln!("no {name} answer; skipping");
        return;
    };
    let ours = comparable_lines(tsx);
    let theirs = comparable_lines(&answer);
    let mut only_ours = ours.clone();
    for line in &theirs {
        if let Some(index) = only_ours.iter().position(|other| other == line) {
            only_ours.remove(index);
        }
    }
    let mut only_theirs = theirs.clone();
    for line in &ours {
        if let Some(index) = only_theirs.iter().position(|other| other == line) {
            only_theirs.remove(index);
        }
    }
    let unexplained = |lines: &[String]| {
        lines
            .iter()
            .filter(|line| !module_wrapper(line))
            .filter(|line| !differs_on_purpose.iter().any(|(known, _)| known == line))
            .cloned()
            .collect::<Vec<_>>()
    };
    let ours_unexplained = unexplained(&only_ours);
    let theirs_unexplained = unexplained(&only_theirs);
    assert!(
        ours_unexplained.is_empty() && theirs_unexplained.is_empty(),
        "{name}: lines not accounted for.\n  written here and not in the answer:\n    {}\n  in the answer and not here:\n    {}",
        ours_unexplained.join("\n    "),
        theirs_unexplained.join("\n    ")
    );

    let report = validate_devup_ui_tsx(tsx, None, false);
    let errors = report
        .violations
        .iter()
        .filter(|violation| violation.severity == Severity::Error)
        .map(|violation| format!("{}: {}", violation.rule, violation.message))
        .collect::<Vec<_>>();
    assert!(
        report.ok && errors.is_empty(),
        "{name}: {}",
        errors.join("\n")
    );
}

/// `429:1966`, six pictures in a 3×3 grid: the grid's template, and each
/// picture's cell where it is not the next one.
#[test]
fn the_grid_of_pictures_is_the_plugin_s() {
    matches_the_answer("grid", &[]);
}

/// `446:1971`, the report section: a translucent gradient between two tokens
/// on the backdrop and on each card, a text with a gradient fill, and an
/// illustration pinned below its frame through its children's constraints.
#[test]
fn the_report_section_is_the_plugin_s() {
    matches_the_answer(
        "report",
        &[
            // The illustration is a folded asset drawn at a size, and the
            // plugin writes no height for a positioned frame with children;
            // this keeps it, as the puzzle icon and the spinner frames keep
            // theirs. See `codegen::layout`.
            ("h=\"734.76px\"", "a pinned size is a layout fact"),
            // Three cards hold an `Icons` instance each at a different
            // variant - analysis, time, thunder - and the answer names all
            // three `/icons/Icons.svg`, one file overwriting the next, so
            // every card draws the chart. The render harness showed it.
            // Here an instance whose layer name another variant shares
            // carries its variant. See `codegen::style::asset_stem`.
            (
                "maskImage=\"url(/icons/Icons.svg)\"",
                "three icons are three files",
            ),
            (
                "maskImage=\"url(/icons/Icons=analysis.svg)\"",
                "three icons are three files",
            ),
            (
                "maskImage=\"url(/icons/Icons=time.svg)\"",
                "three icons are three files",
            ),
            (
                "maskImage=\"url(/icons/Icons=thunder.svg)\"",
                "three icons are three files",
            ),
        ],
    );
}

/// `422:6865`, the notice screen at its desktop width, with the header and
/// footer expanded to primitives: the plugin's Pure Code tab, against the
/// same frame taken out of the three-width capture.
#[test]
fn the_notice_desktop_is_the_plugin_s_pure_code() {
    let Some(tsx) = ours_of("bp-family", Some("422:6865")) else {
        eprintln!("no notice capture; skipping");
        return;
    };
    compare(
        "notice",
        &tsx,
        &[
            // The theme toggle's knob is a 28px frame that lays nothing out,
            // holding one 20px icon the designer centred; it is a `Center`.
            // The answer's `Flex` with `p="4px"` is a layout Figma inferred
            // when the answer was written and no longer reports for the frame
            // (`inferredAutoLayout: null` in the capture) — the same thing
            // `popup`'s answer shows.
            (
                "<Center aspectRatio=\"1\" bg=\"#2D2926\" borderRadius=\"10000px\" boxSize=\"28px\">",
                "the centred icon is centred, not padded",
            ),
            ("</Center>", "the centred icon is centred, not padded"),
            ("<Flex", "inferred layout the file no longer has"),
            (">", "inferred layout the file no longer has"),
            ("</Flex>", "inferred layout the file no longer has"),
            (
                "aspectRatio=\"1\"",
                "inferred layout the file no longer has",
            ),
            ("bg=\"#2D2926\"", "inferred layout the file no longer has"),
            (
                "borderRadius=\"10000px\"",
                "inferred layout the file no longer has",
            ),
            ("boxSize=\"28px\"", "inferred layout the file no longer has"),
            ("p=\"4px\"", "inferred layout the file no longer has"),
            // The footer's address breaks after the CEO's name with a soft
            // return, U+2028, which is drawn as `<br />`; the plugin passes
            // the character through, and the answer has it as a space.
            (
                "대표이사 : 이석중<br />주소 : 13840 경기 과천시 과천대로7나길 60 과천어반허브, C동 5층/6층<br />TEL : 1899-3058<br />FAX : 02-3318-3351<br />이메일 : sales@laonpeople.com{\" \"}",
                "a soft return is drawn",
            ),
            (
                "대표이사 : 이석중 주소 : 13840 경기 과천시 과천대로7나길 60 과천어반허브, C동 5층/6층<br />TEL : 1899-3058<br />FAX : 02-3318-3351<br />이메일 : sales@laonpeople.com{\" \"}",
                "a soft return is drawn",
            ),
        ],
    );
}
