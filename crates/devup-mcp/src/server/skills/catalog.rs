
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::skills::{RawGithubSkillUpstream, SkillFetchError, SkillProvenance, SkillUpstream};
use serde::Deserialize;

const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);
const OFFLINE_ENV: &str = "DEVUP_MCP_SKILLS_OFFLINE";

// Read once per process. Client construction and fetching happen only on an
// explicit install, never while constructing a server or running self-check.
static OFFLINE: LazyLock<bool> = LazyLock::new(|| {
    std::env::var(OFFLINE_ENV).is_ok_and(|value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false"
        )
    })
});

struct InstallUpstream;

#[async_trait::async_trait]
impl SkillUpstream for InstallUpstream {
    async fn fetch(
        &self,
        source_url: &str,
    ) -> Result<crate::skills::FetchedSkill, SkillFetchError> {
        if *OFFLINE {
            return Err(SkillFetchError::Network(format!(
                "{OFFLINE_ENV} disables fetching"
            )));
        }
        static UPSTREAM: LazyLock<Result<RawGithubSkillUpstream, String>> = LazyLock::new(|| {
            RawGithubSkillUpstream::with_timeout(FETCH_TIMEOUT).map_err(|error| error.to_string())
        });
        UPSTREAM
            .as_ref()
            .map_err(|error| SkillFetchError::Network(error.clone()))?
            .fetch(source_url)
            .await
    }
}

pub const MIME_TYPE: &str = "text/markdown";

/// The registry. Parsed rather than duplicated into consts so that refreshing a
/// skill touches one file a script can write, instead of a JSON file and a Rust
/// literal that can disagree about which commit is in the binary.
pub const MANIFEST_JSON: &str = include_str!("manifest.json");

/// Skill name to its documents, for the origins that carry text. `include_str!`
/// needs a literal path, so this is the one place the set is spelled out;
/// [`tests::every_origin_is_well_formed`] holds it to the manifest in both
/// directions.
///
/// The inner key is the document's path relative to the skill directory, and it
/// is deliberately the same string on both sides: `references/theme-colors.md`
/// is read from `skills/devfive-frontend/references/theme-colors.md` here and
/// written to `<root>/devfive-frontend/references/theme-colors.md` on install.
/// A skill whose list is one `SKILL.md` is the single-file case, not a
/// different one.
type Documents = &'static [(&'static str, &'static str)];

const EMBEDDED: &[(&str, Documents)] = &[
    (
        "figma-bridge-setup",
        &[("SKILL.md", include_str!("figma-bridge-setup/SKILL.md"))],
    ),
    (
        "devup-ui",
        &[("SKILL.md", include_str!("devup-ui/SKILL.md"))],
    ),
    (
        "devfive-frontend",
        &[
            ("SKILL.md", include_str!("devfive-frontend/SKILL.md")),
            (
                "references/critical-rules.md",
                include_str!("devfive-frontend/references/critical-rules.md"),
            ),
            (
                "references/anti-patterns.md",
                include_str!("devfive-frontend/references/anti-patterns.md"),
            ),
            (
                "references/common-patterns.md",
                include_str!("devfive-frontend/references/common-patterns.md"),
            ),
            (
                "references/theme-colors.md",
                include_str!("devfive-frontend/references/theme-colors.md"),
            ),
        ],
    ),
    (
        "devup-implementation-plan",
        &[(
            "SKILL.md",
            include_str!("devup-implementation-plan/SKILL.md"),
        )],
    ),
    (
        "devup",
        &[("SKILL.md", include_str!("devup/SKILL.md"))],
    ),
    (
        "changepacks",
        &[("SKILL.md", include_str!("changepacks/SKILL.md"))],
    ),
    (
        "vespera",
        &[("SKILL.md", include_str!("vespera/SKILL.md"))],
    ),
    (
        "vespertide",
        &[("SKILL.md", include_str!("vespertide/SKILL.md"))],
    ),
];

/// The document every skill loader opens first. A skill is installed when this
/// file is on disk; the rest are what it links to.
pub const ENTRY_DOCUMENT: &str = "SKILL.md";

/// How each provenance note opens. Named so the writer and the reader cannot
/// drift apart: [`Skill::provenance_note`] builds them and
/// [`Skill::freshness_at`] recognises a file by them.
const FETCHED_MARKER: &str = "<!-- Fetched from ";
const AUTHORED_MARKER: &str = "<!-- Authored in ";
const VENDORED_MARKER: &str = "<!-- Vendored from ";

/// How the copy on disk compares with the one this binary would write.
///
/// `installed` used to mean `is_file()` and nothing more, so a `SKILL.md`
/// written by a devup-mcp from six months ago - or a one-line placeholder
/// someone dropped there - was indistinguishable from the current document.
/// Worse, `install` skipped anything already present, which meant there was no
/// way to update a skill at all: the first install a machine ever did was the
/// last one it would get.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Byte-identical to what an install would write now, or fetched from
    /// upstream - which is at least as new as the copy in this binary, and
    /// cannot be compared against it without the network.
    Current,
    /// Carries devup-mcp's provenance, but is not what this build writes.
    /// Safe to replace, because devup-mcp wrote it.
    Older,
    /// No devup-mcp provenance note at all. Someone wrote this, or it came
    /// from somewhere else. Never overwritten - reported instead, because
    /// replacing it would destroy work this server did not do.
    Foreign,
    /// The entry document is exactly ours, but a file it links to is missing
    /// or has different bytes. This is the shape that looks installed and is
    /// not: a `SKILL.md` whose references go nowhere.
    Incomplete,
}

impl Freshness {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Older => "older",
            Self::Foreign => "foreign",
            Self::Incomplete => "incomplete",
        }
    }

    /// Whether an install should rewrite this copy.
    ///
    /// `Foreign` is excluded on purpose. Someone put their own document there
    /// and an install is not the moment to decide it was a mistake.
    pub fn should_refresh(self) -> bool {
        matches!(self, Self::Older | Self::Incomplete)
    }
}

/// Where agent runtimes keep project-local skills, in the order they are
/// preferred when none exists yet.
///
/// Project-local rather than the user's home directory on purpose: the project
/// root is already the only place devup-mcp is allowed to write, so installing
/// here needs no new permission, and the skills travel with the repository
/// instead of being a thing each machine has to be told about separately.
///
/// Each entry is a directory holding one subdirectory per skill, each with a
/// `SKILL.md` inside - the layout every one of these runtimes reads.
pub const SKILL_ROOTS: &[&str] = &[".claude/skills", ".opencode/skill", ".agents/skills"];

/// Where the same runtimes keep skills that apply to every project on the
/// machine, relative to the user's home directory.
///
/// A separate list rather than `SKILL_ROOTS` joined onto the home directory,
/// because the two disagree: opencode reads `~/.config/opencode/skill`, not
/// `~/.opencode/skill`. Joining would have looked right and found nothing.
///
/// Read, never written. devup-mcp writes only inside an allowed output root
/// and the home directory is not one - that boundary stays. But a skill
/// already installed here *is* loaded by the runtime, and reporting it missing
/// is what makes an agent write a second copy that then drifts from the first.
pub const USER_SKILL_ROOTS: &[&str] = &[
    ".claude/skills",
    ".codex/skills",
    ".config/opencode/skill",
    ".agents/skills",
    ".cursor/skills",
    ".codeium/windsurf/skills",
    ".copilot/skills",
    ".gemini/skills",
    ".cline/skills",
    ".continue/skills",
    ".config/agents/skills",
    ".config/amp/skills",
];

/// Every project-local convention devup-mcp knows of, for a client that did
/// not say which it is.
///
/// Read, not written. An unidentified client still has whatever someone
/// installed before, and finding it is what stops a second copy being written
/// beside it; [`Runtime::write_roots`] is the narrower list an install uses.
const EVERY_PROJECT_ROOT: &[&str] = &[
    ".claude/skills",
    ".opencode/skill",
    ".agents/skills",
    ".cursor/skills",
    ".windsurf/skills",
    ".github/skills",
    ".gemini/skills",
    ".cline/skills",
    ".continue/skills",
    ".goose/skills",
    ".codex/skills",
];

/// Which agent runtime is driving this server, from the MCP `clientInfo` name.
///
/// It has to be known because the runtimes do not read the same directory, and
/// installing into one another runtime never opens is a *silent* failure: the
/// call reports success, the file is on disk, and the skill still never loads.
///
/// That is the exact shape of the reports from Codex. A fresh project has no
/// skill root at all, so the choice fell through to `SKILL_ROOTS[0]` -
/// `.claude/skills` - and Codex reads `.agents/skills`, never that. The agent
/// was told the skill was installed and went on writing devup-ui from guesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    ClaudeCode,
    Codex,
    Opencode,
    Cursor,
    Zed,
    Windsurf,
    VsCode,
    GeminiCli,
    Cline,
    Continue,
    Amp,
    Goose,
    Unknown,
}

impl Runtime {
    /// Recognises the client from the name it sent at `initialize`.
    ///
    /// Order matters where one name contains another: `cursor-vscode` is
    /// Cursor and not VS Code, and `opencode` has to be tested before the
    /// shorter editor names. Short, common words are matched exactly or with
    /// their own prefix rather than as a bare substring - `amp` alone would
    /// claim any client with "example" or "amplify" in its name, and `zed`
    /// would claim anything "analyzed".
    ///
    /// A name this does not know is not a failure. It falls to `Unknown`,
    /// which reads every convention and writes the three broad ones, so the
    /// worst outcome is a spare directory rather than a skill nothing loads.
    pub fn from_client_name(name: &str) -> Self {
        let name = name.trim().to_ascii_lowercase();
        let has = |needle: &str| name.contains(needle);
        if has("opencode") {
            Self::Opencode
        } else if has("cursor") {
            Self::Cursor
        } else if has("codex") {
            Self::Codex
        } else if has("claude") {
            Self::ClaudeCode
        } else if has("windsurf") {
            Self::Windsurf
        } else if has("gemini") {
            Self::GeminiCli
        } else if has("cline") {
            Self::Cline
        } else if has("continue") {
            Self::Continue
        } else if has("goose") {
            Self::Goose
        } else if name.starts_with("zed") {
            // Anchored, not a substring: `analyzed-client` contains "zed-".
            Self::Zed
        } else if has("ampcode") || has("amp-mcp") {
            Self::Amp
        } else if has("visual studio code") || has("vscode") || has("code - oss") {
            Self::VsCode
        } else {
            Self::Unknown
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::Cursor => "cursor",
            Self::Zed => "zed",
            Self::Windsurf => "windsurf",
            Self::VsCode => "vscode",
            Self::GeminiCli => "gemini-cli",
            Self::Cline => "cline",
            Self::Continue => "continue",
            Self::Amp => "amp",
            Self::Goose => "goose",
            Self::Unknown => "unknown",
        }
    }

    /// The project-local roots this runtime loads from, most preferred first.
    ///
    /// `.agents/skills` has become the shared convention - Codex, Zed, Amp and
    /// Goose read it as their primary, and Cursor, Windsurf, VS Code and
    /// Gemini read it alongside their own. Cline is the exception that makes
    /// the table worth having: it reads `.agents/skills` only at the user
    /// level, so a project-local copy there is one nothing opens.
    ///
    /// `Unknown` reads every convention, so an existing install is found
    /// whoever wrote it; what it *writes* is narrower, see [`Self::write_roots`].
    pub fn project_roots(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => &[".claude/skills"],
            Self::Codex => &[".agents/skills"],
            Self::Opencode => &[".opencode/skill", ".claude/skills", ".agents/skills"],
            Self::Cursor => &[
                ".cursor/skills",
                ".agents/skills",
                ".claude/skills",
                ".codex/skills",
            ],
            Self::Zed => &[".agents/skills"],
            Self::Windsurf => &[".windsurf/skills", ".agents/skills", ".claude/skills"],
            Self::VsCode => &[".github/skills", ".claude/skills", ".agents/skills"],
            Self::GeminiCli => &[".gemini/skills", ".agents/skills"],
            // No project-local `.agents/skills`: Cline reads that one only
            // from the home directory.
            Self::Cline => &[".cline/skills"],
            Self::Continue => &[".continue/skills", ".claude/skills"],
            Self::Amp => &[".agents/skills", ".claude/skills"],
            // Documented as the current convention, with `.goose/skills` kept
            // for compatibility. Goose renamed this feature mid-release and
            // users report the global path behaving differently across
            // versions, so this arm is the least certain in the table.
            Self::Goose => &[".agents/skills", ".goose/skills", ".claude/skills"],
            Self::Unknown => EVERY_PROJECT_ROOT,
        }
    }

    /// Where an install writes when no root exists yet.
    ///
    /// The same as [`Self::project_roots`] for a known client - the first
    /// entry is its own convention. `Unknown` is the one that differs: it
    /// reads every convention but writes only the three broad ones, because
    /// writing a copy into each of a dozen editor-specific directories would
    /// be worse than the problem it solves.
    pub fn write_roots(self) -> &'static [&'static str] {
        match self {
            Self::Unknown => SKILL_ROOTS,
            other => other.project_roots(),
        }
    }

    /// The machine-wide roots this runtime loads from. Read, never written.
    ///
    /// `~/.codex/skills` is deprecated upstream but still scanned, so a skill
    /// sitting there is loaded and must not be reported missing. The same
    /// reasoning is why every client here lists its own home directory: a
    /// Cursor user who installed once into `~/.cursor/skills` was being told
    /// the skill was missing, and told to write a second copy.
    pub fn user_roots(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => &[".claude/skills"],
            Self::Codex => &[".agents/skills", ".codex/skills"],
            Self::Opencode => &[".config/opencode/skill", ".claude/skills", ".agents/skills"],
            Self::Cursor => &[
                ".cursor/skills",
                ".agents/skills",
                ".claude/skills",
                ".codex/skills",
            ],
            Self::Zed => &[".agents/skills"],
            Self::Windsurf => &[
                ".codeium/windsurf/skills",
                ".agents/skills",
                ".claude/skills",
            ],
            Self::VsCode => &[".copilot/skills", ".claude/skills", ".agents/skills"],
            Self::GeminiCli => &[".gemini/skills", ".agents/skills"],
            Self::Cline => &[".cline/skills", ".agents/skills"],
            Self::Continue => &[".continue/skills", ".claude/skills"],
            Self::Amp => &[
                ".config/agents/skills",
                ".agents/skills",
                ".config/amp/skills",
                ".claude/skills",
            ],
            Self::Goose => &[".agents/skills", ".claude/skills"],
            Self::Unknown => USER_SKILL_ROOTS,
        }
    }
}

/// The user's home directory, or `None` where the platform does not say.
///
/// `HOME` first so a deliberate override wins on every platform; `USERPROFILE`
/// is the Windows spelling and is the one actually set there.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Everything it takes to answer "is this skill installed, and where would it
/// go": the project, the machine-wide location, and which runtime reads which.
///
/// One value rather than three parameters threaded through every function,
/// and `home` is a field rather than a call so a test can say `None` and get
/// an answer that does not depend on the developer's own machine - where
/// devup-ui is very likely already installed in `~/.claude/skills`, which is
/// enough to turn "reports missing correctly" into a test that passes in CI
/// and fails on the laptop of the person who wrote it.
#[derive(Debug, Clone)]
pub struct Lookup {
    pub project: PathBuf,
    pub home: Option<PathBuf>,
    pub runtime: Runtime,
    /// The name the client sent, kept verbatim beside the verdict drawn from
    /// it.
    ///
    /// Half the table is matched against names observed in the wild rather
    /// than read out of source, because those clients are closed. When such a
    /// guess is wrong the match simply does not fire and the runtime reads
    /// `unknown` - safe, and completely silent. Echoing the name is what turns
    /// that into something a reader can see and report, and it is the only way
    /// the table gets corrected.
    pub client_name: Option<String>,
}

impl Lookup {
    /// The project a caller named, or the server's own write root when they
    /// named none.
    ///
    /// The fallback is not the project unless the two were configured to be
    /// the same. A host that granted one shared parent - an Orca worktree
    /// pool, a monorepo checkout - had every call reporting on that parent,
    /// where no runtime looks for skills and an install would land somewhere
    /// nothing reads.
    pub fn new(project_root: Option<&str>, fallback: &Path, client_name: Option<&str>) -> Self {
        Self {
            project: project_root.map_or_else(|| fallback.to_path_buf(), PathBuf::from),
            home: home(),
            runtime: client_name.map_or(Runtime::Unknown, Runtime::from_client_name),
            client_name: client_name.map(str::to_owned),
        }
    }

    /// A lookup that sees no machine-wide roots.
    ///
    /// Tests only: the production paths always read the real home, because a
    /// skill installed there really is loaded and really must not be reported
    /// missing.
    #[cfg(test)]
    pub fn project_only(project: PathBuf, runtime: Runtime) -> Self {
        Self {
            project,
            home: None,
            runtime,
            client_name: None,
        }
    }

    /// Every machine-wide root this runtime reads that exists, preferred
    /// first.
    pub fn user_roots(&self) -> Vec<PathBuf> {
        let Some(home) = &self.home else {
            return Vec::new();
        };
        self.runtime
            .user_roots()
            .iter()
            .map(|root| join_root(home, root))
            .filter(|path| path.is_dir())
            .collect()
    }

    /// Which project-local roots already exist, in preference order.
    ///
    /// Existence is the signal. A repository that already has `.claude/skills`
    /// has answered the question of which runtime it is for, and guessing
    /// differently would install into a directory nothing reads.
    pub fn existing_roots(&self) -> Vec<PathBuf> {
        self.runtime
            .project_roots()
            .iter()
            .map(|root| join_root(&self.project, root))
            .filter(|path| path.is_dir())
            .collect()
    }

    /// The roots an install writes to: the first that already exists, else the
    /// conventions this runtime reads.
    ///
    /// A list rather than one path because of the `Unknown` case. With no
    /// client name and no existing root there is nothing to choose on, and
    /// picking one of three is a two-in-three chance of writing where nothing
    /// looks. Writing all three is the only answer that is right without
    /// knowing.
    pub fn target_roots(&self) -> Vec<PathBuf> {
        let existing = self.existing_roots();
        if !existing.is_empty() {
            return existing.into_iter().take(1).collect();
        }
        let conventions = self.runtime.write_roots();
        let chosen = if self.runtime == Runtime::Unknown {
            conventions
        } else {
            &conventions[..1]
        };
        chosen
            .iter()
            .map(|root| join_root(&self.project, root))
            .collect()
    }

    /// Every place this skill is installed *and this runtime would load it
    /// from*.
    ///
    /// Every root that runtime reads, not just the preferred one: a skill put
    /// by hand into `.agents/skills` is installed, and reporting it missing
    /// would have the agent write a second copy that then drifts from the
    /// first.
    ///
    /// Scoped to the runtime rather than to every known root, because the two
    /// mistakes are symmetric and both silent. A file in a directory this
    /// runtime never opens is not loaded, and counting it says "installed"
    /// about a skill the agent will never see.
    ///
    /// The machine-wide roots are read for the same reason, and it is not a
    /// hypothetical: on a machine with devup-ui in `~/.agents/skills`,
    /// `~/.claude/skills`, `~/.codex/skills` and `~/.config/opencode/skill` -
    /// all four loaded by their runtimes - this reported `installedCount: 0`.
    /// The README calls telling someone to install what they already have the
    /// noise that teaches them to ignore the field, and that is what it was.
    pub fn installed_paths(&self, name: &str) -> Vec<PathBuf> {
        let mut paths = self
            .runtime
            .project_roots()
            .iter()
            .map(|root| install_path(&join_root(&self.project, root), name))
            .chain(
                self.user_roots()
                    .into_iter()
                    .map(|root| install_path(&root, name)),
            )
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        // A project that *is* the home directory reaches the same file twice.
        paths.sort();
        paths.dedup();
        paths
    }
}

/// Joins a `SKILL_ROOTS` entry onto a project directory one component at a
/// time.
///
/// `Path::join` treats `".claude/skills"` as a single component and keeps the
/// forward slash verbatim, so on Windows the reported path came back as
/// `C:\work\.claude/skills\devup-ui\SKILL.md`. It opens either way, but a path
/// a reader has to squint at is one they cannot confidently compare to what
/// their editor shows them.
fn join_root(project: &Path, root: &str) -> PathBuf {
    root.split('/')
        .fold(project.to_path_buf(), |path, part| path.join(part))
}

/// The byte offset just past a leading YAML frontmatter block, if there is one.
///
/// Only a `---` line at byte zero opens a block, which is what every skill
/// loader requires; a `---` further down is a horizontal rule and closing on it
/// would cut the document in half.
fn frontmatter_end(text: &str) -> Option<usize> {
    let rest = text
        .strip_prefix("---\n")
        .or(text.strip_prefix("---\r\n"))?;
    let opened = text.len() - rest.len();
    let mut offset = opened;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if matches!(line.trim_end_matches(['\r', '\n']), "---") {
            return Some(offset);
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Copied from another DevFive repository into this binary; installable
    /// with no network, and pins the revision it copied.
    Embedded,
    /// Written in this repository. Installable with no network like an embedded
    /// skill, but pins no upstream revision because it has no upstream.
    Own,
    /// Someone else's, installed from source by the agent.
    External,
}

impl Origin {
    /// The word used for this origin everywhere a caller can read it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::Own => "own",
            Self::External => "external",
        }
    }

    /// Whether devup-mcp carries this skill's text and can therefore write it.
    pub fn is_carried(self) -> bool {
        matches!(self, Self::Embedded | Self::Own)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRecord {
    pub name: String,
    pub origin: Origin,
    pub title: String,
    pub description: String,
    /// Which devup-mcp output or input this project's rules govern - the reason
    /// to install it, rather than a second copy of the description.
    pub used_for: String,
    pub repo: String,
    pub path: String,
    pub source_url: String,
    pub latest_url: String,

    /// Every file this skill installs. Present for the origins devup-mcp
    /// carries, absent for external.
    #[serde(default)]
    pub documents: Vec<DocumentRecord>,

    // Embedded only: an `own` skill has no upstream revision to pin.
    pub commit: Option<String>,
    pub committed_at: Option<String>,

    // External only.
    pub install_command: Option<String>,
    pub license: Option<String>,
    pub license_note: Option<String>,
}

/// One file of a skill, and the bytes it is supposed to be.
///
/// The digest is what makes a hand-edit of a vendored copy fail the build
/// instead of shipping quietly, so it is recorded per file rather than once per
/// skill: a five-document skill whose fourth reference was edited has to fail
/// on that reference, naming it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRecord {
    /// Relative to the skill directory, on both sides: the path under
    /// `skills/<name>/` here and under `<root>/<name>/` once installed.
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    skills: Vec<SkillRecord>,
}

pub struct Skill {
    pub record: SkillRecord,
    pub uri: String,
    /// The MCP resource name. Prefixed so it cannot collide with the usage
    /// guide or with a generated output manifest.
    pub resource_name: String,
    /// `Some` for the origins devup-mcp carries, `None` for external.
    pub texts: Option<Documents>,
}

static SKILLS: LazyLock<Vec<Skill>> = LazyLock::new(|| {
    let manifest: Manifest =
        serde_json::from_str(MANIFEST_JSON).expect("the vendored skill manifest is valid JSON");
    manifest
        .skills
        .into_iter()
        .map(|record| {
            let texts = record.origin.is_carried().then(|| {
                EMBEDDED
                    .iter()
                    .find(|(name, _)| *name == record.name)
                    .map(|(_, documents)| *documents)
                    .expect("every carried manifest entry has documents")
            });
            Skill {
                uri: uri_for(&record.name),
                resource_name: format!("devup-skill-{}", record.name),
                texts,
                record,
            }
        })
        .collect()
});

pub fn uri_for(name: &str) -> String {
    format!("devup://skill/{name}")
}

pub fn all() -> &'static [Skill] {
    &SKILLS
}

pub fn find_by_uri(uri: &str) -> Option<&'static Skill> {
    SKILLS.iter().find(|skill| skill.uri == uri)
}

pub fn find_by_name(name: &str) -> Option<&'static Skill> {
    SKILLS.iter().find(|skill| skill.record.name == name)
}

/// Where one of a skill's documents lives under a given root.
///
/// `relative` is split on `/` for the same reason [`join_root`] splits: a
/// `references/theme-colors.md` joined whole would keep its forward slash and
/// report `...\devfive-frontend\references/theme-colors.md` on Windows.
pub fn document_path(root: &Path, name: &str, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.join(name), |path, part| path.join(part))
}

/// Where a skill's `SKILL.md` lives under a given root.
pub fn install_path(root: &Path, name: &str) -> PathBuf {
    document_path(root, name, ENTRY_DOCUMENT)
}

struct FetchedDocuments {
    documents: Vec<(&'static str, String)>,
    provenance: Vec<serde_json::Value>,
}
