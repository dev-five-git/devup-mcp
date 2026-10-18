

/// As [`ask_shape`], with `all` the whole plan's questions so a conditional
/// question can say in its own text when it applies - the flat list may be
/// asked in one call, where the answer it depends on is not yet known.
fn ask_shape_with(all: &[Value], questions: &[Value]) -> Vec<Value> {
    questions
        .iter()
        .map(|question| {
            let mut shaped = question.clone();
            let conditions = array(&question["when"])
                .iter()
                .map(|condition| {
                    let parent = all
                        .iter()
                        .find(|other| other["id"] == condition["question"])
                        .map_or_else(
                            || text(&condition["question"]).to_owned(),
                            |other| text(&other["header"]).to_owned(),
                        );
                    format!("\"{parent}\" is \"{}\"", text(&condition["answer"]))
                })
                .collect::<Vec<_>>();
            if !conditions.is_empty() {
                shaped["question"] = json!(format!(
                    "{} (Only if {}.)",
                    text(&question["question"]),
                    conditions.join(" and ")
                ));
            }
            if let Some(object) = shaped.as_object_mut() {
                object.remove("resolve");
                object.remove("kind");
                object.remove("fix");
                if let Some(conditions) = object.get_mut("when").and_then(Value::as_array_mut) {
                    for condition in conditions {
                        if let Some(condition) = condition.as_object_mut() {
                            condition.remove("action");
                        }
                    }
                }
            }
            shaped
        })
        .collect()
}

#[derive(Debug, Clone)]
struct Frame {
    node_id: String,
    name: String,
    width: f64,
    height: Option<f64>,
    section: Option<String>,
    section_name: Option<String>,
}

impl Frame {
    fn json(&self) -> Value {
        json!({"nodeId": self.node_id, "name": self.name, "width": self.width, "height": self.height,
            "section": self.section, "sectionName": self.section_name})
    }
}

/// How the frames of one name are turned into pages.
#[derive(Debug, Clone, PartialEq)]
enum Grouping {
    /// Decided: one page, from a single frame, project config, or evidence.
    Fixed(&'static str),
    /// Several widths in one section: asked, one responsive page recommended.
    Ask {
        recommend_single: bool,
        reason: String,
    },
    /// The same screen name in different sections. A designer usually does
    /// that for a reason, so this is never resolved here: the question names
    /// the sections and frames and offers to have the Figma fixed instead.
    Sections {
        all_device: bool,
        sections: Vec<String>,
        why: String,
    },
}

#[derive(Debug, Clone)]
struct DraftScreen {
    frames: Vec<Frame>,
    words: Vec<String>,
    parts: Vec<String>,
    links: Vec<PlanLinkInput>,
    grouping: Grouping,
    /// Conditions every question about this screen carries: a structure
    /// question about it must have been answered with "keep" for any of them
    /// to apply.
    extra_when: Vec<Value>,
    /// The structure questions this screen is part of, so confirm can tell
    /// whether the user asked for a Figma fix instead.
    structure: Vec<String>,
}

impl DraftScreen {
    fn id(&self) -> &str {
        &self.frames[0].node_id
    }
    fn name(&self) -> &str {
        &self.frames[0].name
    }
    fn widths(&self) -> Vec<u32> {
        widths_of(&self.frames)
    }
    /// The question that decides how this screen's frames become pages.
    fn grouping_question(&self) -> Option<String> {
        match self.grouping {
            Grouping::Fixed(_) => None,
            Grouping::Ask { .. } => Some(format!("group:{}", self.id())),
            Grouping::Sections { .. } => Some(format!("sections:{}", self.id())),
        }
    }
}

fn widths_of(frames: &[Frame]) -> Vec<u32> {
    let mut widths = frames
        .iter()
        .map(|frame| frame.width.round().max(0.0) as u32)
        .collect::<Vec<_>>();
    widths.sort_unstable_by(|a, b| b.cmp(a));
    widths.dedup();
    widths
}

fn width_list(widths: &[u32]) -> String {
    widths
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether a section name says only which device it is for: `PC`,
/// `Mobile`, `Tablet`, `모바일`.
fn device_name(name: &str) -> bool {
    let lowered = name.to_lowercase();
    let words = lowered
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    !words.is_empty()
        && words.iter().all(|word| {
            DEVICE_WORDS.contains(word)
                || SECTION_DEVICE_WORDS.contains(word)
                || word.chars().all(|c| c.is_ascii_digit())
        })
}

fn section_label(frame: &Frame) -> String {
    frame
        .section_name
        .clone()
        .or_else(|| frame.section.clone())
        .unwrap_or_else(|| "(no section)".to_owned())
}

fn describe_frames(frames: &[Frame]) -> String {
    frames
        .iter()
        .map(|frame| {
            format!(
                "\"{}\" ({}, {}px, section {})",
                frame.name,
                frame.node_id,
                frame.width.round(),
                section_label(frame)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Frames that share a base name are candidates for one page. Whether they
/// are one is not assumed:
///
/// - several widths in one section that can merge into one responsive tree
///   are asked, recommending one responsive page; `widthGrouping` in the
///   project config decides this case instead of asking;
/// - the same name in different sections - device-named sections included -
///   is always asked, with the facts and a "fix the Figma" option, because
///   that is a designer's structure and not this server's to correct;
/// - the same name at the same width in one section is not grouped; it is
///   flagged as a possible duplicate by [`structure_questions`].
fn group_screens(screens: &[PlanScreenInput], config: &PlanConfig) -> Vec<DraftScreen> {
    let mut order = Vec::<String>::new();
    let mut groups = BTreeMap::<String, Vec<&PlanScreenInput>>::new();
    for screen in screens {
        let key = base_words(&screen.name).join(" ");
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(screen);
    }
    let make = |members: &[&PlanScreenInput], grouping: Grouping| {
        let mut frames = members
            .iter()
            .map(|screen| Frame {
                node_id: screen.node_id.clone(),
                name: screen.name.clone(),
                width: screen.width,
                height: screen.height,
                section: screen.section.clone(),
                section_name: screen.section_name.clone(),
            })
            .collect::<Vec<_>>();
        frames.sort_by(|a, b| b.width.total_cmp(&a.width));
        let mut parts = Vec::<String>::new();
        let mut links = Vec::new();
        for screen in members {
            for part in &screen.parts {
                if !parts.contains(part) {
                    parts.push(part.clone());
                }
            }
            links.extend(screen.links.iter().cloned());
        }
        DraftScreen {
            frames,
            words: base_words(&members[0].name),
            parts,
            links,
            grouping,
            extra_when: Vec::new(),
            structure: Vec::new(),
        }
    };
    let mut result = Vec::new();
    for key in order {
        let members = &groups[&key];
        if members.len() == 1 {
            result.push(make(members, Grouping::Fixed("single-frame")));
            continue;
        }
        // Asked of the base name, not the layer name: `Main - 1440` and
        // `Main - 390` are one name across two widths once the width is gone.
        let named = members
            .iter()
            .map(|screen| (key.as_str(), screen.width))
            .collect::<Vec<_>>();
        let family = devup_mcp_devup_ui::codegen::responsive::breakpoint_family_is_possible(&named);
        let sections = members
            .iter()
            .map(|screen| screen.section.as_deref())
            .collect::<BTreeSet<_>>();
        if sections.len() > 1 {
            let probe = make(members, Grouping::Fixed(""));
            let mut names = Vec::<String>::new();
            for frame in &probe.frames {
                let label = section_label(frame);
                if !names.contains(&label) {
                    names.push(label);
                }
            }
            let all_device = probe
                .frames
                .iter()
                .all(|frame| frame.section_name.as_deref().is_some_and(device_name));
            let mut why = vec!["the same name once widths and device words are removed".to_owned()];
            if family {
                why.push(format!(
                    "their widths ({}) form a desktop-to-mobile pattern",
                    width_list(&probe.widths())
                ));
            }
            result.push(make(
                members,
                Grouping::Sections {
                    all_device,
                    sections: names,
                    why: why.join(", and "),
                },
            ));
            continue;
        }
        if !family {
            for member in members {
                result.push(make(&[member], Grouping::Fixed("single-frame")));
            }
            continue;
        }
        match config.width_grouping.as_deref() {
            Some("responsive") => {
                result.push(make(members, Grouping::Fixed("config:widthGrouping")));
            }
            Some("separate") => {
                for member in members {
                    result.push(make(&[member], Grouping::Fixed("config:widthGrouping")));
                }
            }
            _ => result.push(make(
                members,
                Grouping::Ask {
                    recommend_single: true,
                    reason: "drawn at several widths in one section with matching names".into(),
                },
            )),
        }
    }
    result
}

/// The base name without a trailing part number: `Main 1` and `Main (2)`
/// are parts of one screen if anything is.
fn part_key(words: &[String]) -> Option<String> {
    let (last, rest) = words.split_last()?;
    let numbered =
        last.chars().all(|c| c.is_ascii_digit()) && last.parse::<u32>().is_ok_and(|n| n < 100);
    let rest = rest
        .iter()
        .filter(|word| !matches!(word.as_str(), "part" | "p"))
        .cloned()
        .collect::<Vec<_>>();
    (numbered && !rest.is_empty()).then(|| rest.join(" "))
}

const KEEP_SEPARATE: &str = "They are different screens";
const KEEP_OUTSIDE: &str = "It is its own screen";
pub(super) const FIX_FIGMA: &str = "The Figma structure is wrong: I will fix it";

/// One structural oddity: the frames involved, what looks wrong, and the fix
/// the designer would make.
struct Oddity {
    kind: &'static str,
    screens: Vec<usize>,
    facts: String,
    keep: &'static str,
    keep_description: String,
    fix: String,
}

/// Figma structure that a designer may not have meant, flagged in the same
/// batch with a "fix it" option. Nothing here decides which frame is right.
fn structure_questions(drafts: &[DraftScreen]) -> Vec<Oddity> {
    let mut found = Vec::new();
    let singles = drafts
        .iter()
        .enumerate()
        .filter(|(_, screen)| screen.grouping == Grouping::Fixed("single-frame"))
        .collect::<Vec<_>>();
    // The same name and width in one section.
    let mut duplicates = BTreeMap::<(String, Option<String>, u32), Vec<usize>>::new();
    for (index, screen) in &singles {
        let frame = &screen.frames[0];
        duplicates
            .entry((
                screen.words.join(" "),
                frame.section.clone(),
                widths_of(&screen.frames)[0],
            ))
            .or_default()
            .push(*index);
    }
    for indices in duplicates.values().filter(|indices| indices.len() > 1) {
        let frames = indices
            .iter()
            .map(|index| drafts[*index].frames[0].clone())
            .collect::<Vec<_>>();
        found.push(Oddity {
            kind: "duplicate",
            screens: indices.clone(),
            facts: format!(
                "{} frames share one name and one width in one section: {}.",
                frames.len(),
                describe_frames(&frames)
            ),
            keep: KEEP_SEPARATE,
            keep_description: "Each frame is its own screen and gets its own route.".into(),
            fix: format!(
                "Delete the stale copy or rename the frames so each screen has one name: {}.",
                describe_frames(&frames)
            ),
        });
    }
    // One screen split into numbered parts across sections.
    let mut parts = BTreeMap::<String, Vec<usize>>::new();
    for (index, screen) in drafts.iter().enumerate() {
        if let Some(key) = part_key(&screen.words) {
            parts.entry(key).or_default().push(index);
        }
    }
    for indices in parts.values().filter(|indices| indices.len() > 1) {
        let frames = indices
            .iter()
            .flat_map(|index| drafts[*index].frames.clone())
            .collect::<Vec<_>>();
        let sections = frames
            .iter()
            .map(|frame| frame.section.clone())
            .collect::<BTreeSet<_>>();
        if sections.len() < 2 {
            continue;
        }
        found.push(Oddity {
            kind: "split",
            screens: indices.clone(),
            facts: format!(
                "These numbered frames look like parts of one screen spread across sections: {}.",
                describe_frames(&frames)
            ),
            keep: KEEP_SEPARATE,
            keep_description: "Each part is its own screen and gets its own route.".into(),
            fix: format!(
                "Put the parts of one screen into one frame in one section: {}.",
                describe_frames(&frames)
            ),
        });
    }
    // A frame outside any section beside frames that are in sections.
    let sectioned = drafts
        .iter()
        .flat_map(|screen| &screen.frames)
        .filter_map(|frame| frame.section.as_ref().map(|_| section_label(frame)))
        .collect::<BTreeSet<_>>();
    if !sectioned.is_empty() {
        for (index, screen) in &singles {
            let frame = &screen.frames[0];
            if frame.section.is_some() {
                continue;
            }
            found.push(Oddity {
                kind: "loose",
                screens: vec![*index],
                facts: format!(
                    "\"{}\" ({}, {}px) sits outside any section, while the other frames are in {}.",
                    frame.name,
                    frame.node_id,
                    frame.width.round(),
                    sectioned.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
                keep: KEEP_OUTSIDE,
                keep_description: "Plan it as a screen of its own, outside the sections.".into(),
                fix: format!(
                    "Move \"{}\" ({}) into the section it belongs to, or out of this area.",
                    frame.name, frame.node_id
                ),
            });
        }
    }
    found
}

/// Layer names that mark a Figma node as the site's logo or icon, and as its
/// share image. Frames named like this are site assets, not pages.
static LOGO_WORDS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../text/screen-plan-logo-words.json"))
        .expect("vendored keyword JSON")
});
/// Narrow on purpose: a bare "share" or "thumbnail" names real UI - a share
/// sheet, a video thumbnail - and those are screens.
static OG_WORDS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../text/screen-plan-og-words.json"))
        .expect("vendored keyword JSON")
});
/// Package names that say a project already measures or verifies something.
static ANALYTICS_PACKAGES: std::sync::LazyLock<Vec<&'static str>> =
    std::sync::LazyLock::new(|| {
        serde_json::from_str(include_str!("../text/screen-plan-analytics-packages.json"))
            .expect("vendored keyword JSON")
    });
static ANALYTICS_ENV: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../text/screen-plan-analytics-env.json"))
        .expect("vendored keyword JSON")
});

fn asset_kind(name: &str) -> Option<&'static str> {
    if LOGO_WORDS.iter().any(|word| keyword_in(name, word)) || keyword_in(name, "app icon") {
        Some("logo")
    } else if OG_WORDS.iter().any(|word| keyword_in(name, word)) {
        Some("og")
    } else {
        None
    }
}

/// A Figma node that may become the favicon or the share image.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct PlanAssetInput {
    pub node_id: String,
    pub name: String,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
}

/// Files under `directory` (relative to `root`) whose names start with one of
/// `prefixes`, as relative paths.
fn files_named(root: &Path, directory: &str, prefixes: &[&str]) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(join_relative(root, directory)) else {
        return Vec::new();
    };
    let mut found = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            let lowered = name.to_lowercase();
            prefixes.iter().any(|prefix| lowered.starts_with(prefix))
        })
        .map(|name| format!("{directory}/{name}"))
        .collect::<Vec<_>>();
    found.sort();
    found
}

fn join_relative(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .filter(|part| !part.is_empty())
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

fn read_small_up_to(path: &Path, limit: u64) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > limit {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn read_small(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_SOURCE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// The quoted value after `marker` in `source`, e.g. the URL in
/// `metadataBase: new URL("...")`.
fn quoted_after(source: &str, marker: &str) -> Option<String> {
    let rest = &source[source.find(marker)? + marker.len()..];
    let quote_at = rest.find(['"', '\'', '`'])?;
    let quote = rest[quote_at..].chars().next()?;
    let value = &rest[quote_at + 1..];
    let end = value.find(quote)?;
    Some(value[..end].to_owned()).filter(|value| !value.is_empty())
}

/// What the project already says about the site as a whole: its icon, share
/// image, name, URL, analytics, language and not-found page.
fn read_site_facts(root: &Path, evidence: &mut ProjectEvidence) {
    let app = evidence.app_dir.clone();
    let app_root = evidence.app_root().to_owned();
    let public = if app_root == "." {
        "public".to_owned()
    } else {
        format!("{app_root}/public")
    };
    evidence.icon_files = [
        files_named(root, &app, &["icon.", "favicon.", "apple-icon."]),
        files_named(root, &public, &["favicon", "icon.", "apple-touch-icon"]),
    ]
    .concat();
    evidence.og_files = [
        files_named(root, &app, &["opengraph-image.", "twitter-image."]),
        files_named(root, &public, &["og", "share", "opengraph"]),
    ]
    .concat();
    evidence.not_found = ["tsx", "jsx", "js"]
        .iter()
        .map(|ext| format!("{app}/not-found.{ext}"))
        .find(|path| join_relative(root, path).is_file());
    for ext in ["tsx", "jsx", "js"] {
        let layout = format!("{app}/layout.{ext}");
        let Some(source) = read_small(&join_relative(root, &layout)) else {
            continue;
        };
        evidence.lang = quoted_after(&source, "<html lang=").map(|lang| (lang, layout.clone()));
        if let Some(url) = quoted_after(&source, "metadataBase: new URL(") {
            evidence.site_url = Some((url, format!("metadataBase in {layout}")));
        }
        if let Some(template) = quoted_after(&source, "template:")
            && let Some((_, name)) = template.split_once("%s")
        {
            let name = name.trim_matches(|c: char| c.is_whitespace() || "|-·:".contains(c));
            if !name.is_empty() {
                evidence.site_name = Some((name.to_owned(), format!("title template in {layout}")));
            }
        }
        if source.contains("googletagmanager") || source.contains("gtag(") {
            evidence.analytics.push(format!("a Google tag in {layout}"));
        }
    }
    // The lockfile says what is installed, which a range in package.json
    // does not. bun.lock first: bun is DevFive's package manager.
    let bun_lock = read_small_up_to(&root.join("bun.lock"), 16 * 1024 * 1024);
    let npm_lock = read_small_up_to(&root.join("package-lock.json"), 16 * 1024 * 1024)
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    for (name, slot) in [
        ("next", &mut evidence.next_version),
        ("react", &mut evidence.react_version),
        ("vinext", &mut evidence.vinext_version),
    ] {
        if let Some(version) = bun_lock.as_deref().and_then(|lock| bun_locked(lock, name)) {
            *slot = Some((version, "bun.lock".into()));
        } else if let Some(version) = npm_lock
            .as_ref()
            .and_then(|lock| lock["packages"][format!("node_modules/{name}")]["version"].as_str())
        {
            *slot = Some((version.to_owned(), "package-lock.json".into()));
        }
    }
    let env_dirs = if app_root == "." {
        vec![".".to_owned()]
    } else {
        vec![app_root.clone(), ".".to_owned()]
    };
    for file in env_dirs.iter().flat_map(|dir| {
        [".env", ".env.local", ".env.production"]
            .map(|name| join_relative(&PathBuf::from(dir), name))
    }) {
        let Some(source) = read_small(&root.join(&file)) else {
            continue;
        };
        let file = file.display().to_string().replace('\\', "/");
        for line in source.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches(['"', '\'']);
            if evidence.site_url.is_none()
                && matches!(
                    key,
                    "NEXT_PUBLIC_SITE_URL" | "SITE_URL" | "NEXT_PUBLIC_BASE_URL"
                )
                && (value.starts_with("http://") || value.starts_with("https://"))
            {
                evidence.site_url = Some((value.to_owned(), format!("{key} in {file}")));
            }
            if evidence.site_name.is_none()
                && matches!(key, "NEXT_PUBLIC_SITE_NAME" | "SITE_NAME")
                && !value.is_empty()
            {
                evidence.site_name = Some((value.to_owned(), format!("{key} in {file}")));
            }
            if ANALYTICS_ENV.iter().any(|prefix| key.contains(prefix)) {
                evidence.analytics.push(format!("{key} in {file}"));
            }
        }
    }
    // The app's own package.json first: in a workspace the root one holds
    // tooling, the app's holds its framework and React.
    for manifest in env_dirs
        .iter()
        .map(|dir| join_relative(root, dir).join("package.json"))
    {
        let Some(package) =
            read_small(&manifest).and_then(|text| serde_json::from_str::<Value>(&text).ok())
        else {
            continue;
        };
        let shown = manifest
            .strip_prefix(root)
            .unwrap_or(&manifest)
            .display()
            .to_string()
            .replace('\\', "/");
        if evidence.package_name.is_none() {
            evidence.package_name = package["name"].as_str().map(str::to_owned);
        }
        for section in ["dependencies", "devDependencies"] {
            for (name, slot) in [
                ("next", &mut evidence.next_version),
                ("react", &mut evidence.react_version),
                ("vinext", &mut evidence.vinext_version),
            ] {
                if slot.is_none()
                    && let Some(version) = package[section][name].as_str()
                {
                    *slot = Some((version.to_owned(), format!("{shown} {section}")));
                }
            }
            for peer in ["react-dom", "react-server-dom-webpack"] {
                if let Some(version) = package[section][peer].as_str() {
                    evidence
                        .react_peers
                        .push((version.to_owned(), format!("{peer} in {shown} {section}")));
                }
            }
            for name in MOTION_LIBRARIES.iter() {
                if package[section].get(*name).is_some()
                    && !evidence.motion_libraries.iter().any(|known| known == name)
                {
                    evidence.motion_libraries.push((*name).to_owned());
                }
            }
        }
        for section in ["dependencies", "devDependencies"] {
            for name in ANALYTICS_PACKAGES.iter() {
                if package[section].get(*name).is_some() {
                    evidence.analytics.push(format!("{name} in {shown}"));
                }
            }
        }
    }
}

/// The version bun.lock resolved a package to: its entry reads
/// `"react": ["react@19.3.0", ...]`.
fn bun_locked(lock: &str, name: &str) -> Option<String> {
    let marker = format!("\"{name}\": [\"{name}@");
    let start = lock.find(&marker)? + marker.len();
    let end = lock[start..].find('"')? + start;
    Some(lock[start..end].to_owned())
        .filter(|version| version.starts_with(|c: char| c.is_ascii_digit()))
}

/// Libraries that already animate page changes. With one present, adding
/// view transitions is a question, not a default.
static MOTION_LIBRARIES: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../text/screen-plan-motion-libraries.json"))
        .expect("vendored keyword JSON")
});

/// The interaction recipe per element role: what each one gets on hover,
/// press and focus. The devup-implementation-plan skill carries the same
/// table with the reasoning; status counts elements against it per role.
const INTERACTION_RECIPES: &[(&str, &str)] = &[
    (
        "primary button",
        include_str!("../text/screen_plan-INTERACTION_RECIPES-703799bb.md"),
    ),
    (
        "secondary / ghost / outline button",
        "hover a subtle tinted background; active a slightly stronger tint; the same focus ring and disabled state",
    ),
    (
        "text link / \"더 보기 >\" link",
        "hover a colour change or underline; a trailing chevron moves 2px; focus ring",
    ),
    (
        "card that links",
        "hover a 2-4px lift and a shadow token; the image inside zooms to 1.02-1.04 with overflow hidden; active drops the lift; focus ring on the card",
    ),
    (
        "nav item / tab",
        "hover colour; the current route marked with aria-current=\"page\" and the Figma active style (e.g. the underline the header draws); focus ring",
    ),
    (
        "icon button",
        "hover a background circle or tint; active press; aria-label required",
    ),
    (
        "input / select / textarea",
        "hover border; focus border and ring; invalid state with its message; disabled",
    ),
    (
        "accordion / disclosure / dropdown",
        "aria-expanded; the chevron rotates 180deg",
    ),
    (
        "mobile menu / drawer",
        "an open and close transition, when the design has one",
    ),
];

const RESTRAINT_RULES: &str = include_str!("../text/screen_plan-RESTRAINT_RULES-cef67bed.md");

/// `major.minor.patch` from a version or range as package.json writes it:
/// `^16.1.0`, `~19.3.0-canary-...`, `16`. `None` for `latest`,
/// `workspace:*`, `catalog:` and the like, which say nothing about the API.
fn version_triple(value: &str) -> Option<(u64, u64, u64)> {
    let start = value.find(|c: char| c.is_ascii_digit())?;
    if !value[..start]
        .chars()
        .all(|c| matches!(c, '^' | '~' | '>' | '=' | '<' | 'v' | ' '))
    {
        return None;
    }
    let mut parts = value[start..]
        .split(|c: char| !c.is_ascii_digit())
        .take(3)
        .map(|part| part.parse::<u64>().unwrap_or(0));
    Some((
        parts.next()?,
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    ))
}

/// The dependency step every job starts with, word for word as the skills
/// give it. It updates every manifest in the repository - every bun
/// workspace and every Cargo.toml - to the latest versions: `-d` finds them
/// all, `-u` applies the updates as one recoverable batch, and `--rm` removes
/// the lockfiles and installed directories next to each (Cargo.lock and
/// target/ included) so the next install resolves everything fresh.
pub(super) const UPDATE_STEP: &str = "bunx @dependency-check-updates/cli -d -u --rm";

/// The documents the view-transition decision rests on. Read when it was
/// written; the decision names them so a reader can check them again.
///
/// - https://nextjs.org/docs/app/guides/view-transitions - App Router, no
///   config, `import { ViewTransition } from 'react'`, React 19.3.
/// - https://react.dev/reference/react/ViewTransition - the component.
/// - https://vinext.dev/compatibility - Next.js's own
///   app-dir/view-transitions e2e suite passes under vinext (App Router).
/// - <https://github.com/cloudflare/vinext/blob/main/examples/app-router-playground/app/view-transitions/_ui/transitions.tsx>:
///   vinext's own example imports `ViewTransition` from 'react' and sets
///   `experimental.viewTransition: true` in its next config.
/// - <https://github.com/cloudflare/vinext/blob/main/packages/vinext/src/check.ts>:
///   `vinext check` labels that import "partial (passthrough fallback)"
///   whatever the React version; the runtime audit's view-transition check is
///   what settles whether it animates.
/// - https://github.com/cloudflare/vinext#readme - peer dependency react ^19.3.1.
static VIEW_TRANSITION_DOCS: std::sync::LazyLock<Vec<&'static str>> =
    std::sync::LazyLock::new(|| {
        serde_json::from_str(include_str!("../text/screen-plan-view-transition-docs.json"))
            .expect("vendored keyword JSON")
    });

/// The view-transition default: always on. DevFive runs the latest versions
/// (the dependency step comes first), and the latest Next App Router and
/// React support `<ViewTransition>` on vinext and on the `next` CLI alike, so
/// no version is checked. Whether it really animates is not taken on trust
/// either way: the runtime audit's `view-transition` check follows a link and
/// fails when no transition ran. The only "skipped" results are a project
/// that opts out (`viewTransitions: false`) and one that forbids the update
/// (`updateDependencies: false`) while its installed React predates 19.3.
fn view_transition_default(evidence: &ProjectEvidence) -> Value {
    let skipped = |why: String| {
        json!({"item": "view-transitions", "mechanism": "skipped",
            "decision": "No view transitions.", "evidence": why, "docs": *VIEW_TRANSITION_DOCS})
    };
    if evidence.config.view_transitions == Some(false) {
        return skipped(format!("viewTransitions: false in {CONFIG_FILE}"));
    }
    if evidence.config.update_dependencies == Some(false) {
        let react = std::iter::once(&evidence.react_version)
            .flatten()
            .chain(evidence.react_peers.iter())
            .filter_map(|(text, _)| version_triple(text))
            .max();
        if react.is_none_or(|react| react < (19, 3, 0)) {
            return skipped(format!(
                "updateDependencies: false in {CONFIG_FILE} keeps React below 19.3, which has no ViewTransition"
            ));
        }
    }
    let runner = if evidence.framework() == "next"
        && evidence.config.framework.as_deref() != Some("vinext")
    {
        "the next CLI"
    } else {
        "vinext"
    };
    json!({"item": "view-transitions", "mechanism": "react-view-transition",
        "decision": include_str!("../text/screen_plan-view_transition_default-81ca7ab7.md"),
        "evidence": format!("runs on {runner} at the latest versions"), "app": evidence.app_root(), "docs": *VIEW_TRANSITION_DOCS})
}

/// No app, no routes and no framework: a project still to be created.
fn is_new_project(evidence: &ProjectEvidence) -> bool {
    evidence.apps.is_empty()
        && evidence.routes.is_empty()
        && evidence.next_version.is_none()
        && evidence.vinext_version.is_none()
}