//! The skills an agent needs for the code devup-mcp emits, and whether they
//! are installed.
//!
//! devup-mcp hands back devup-ui TSX. On a machine that has devup-mcp and
//! nothing else, the agent receiving that TSX has never seen devup-ui: it does
//! not know that `@devup-ui/react` components are compile-time placeholders,
//! that `$token` refers to `devup.json`, or that a style prop takes a
//! responsive array. It guesses, and the guesses are wrong in ways this server
//! can neither see nor correct.
//!
//! The fix is not to paste the rules into a response. Every agent runtime
//! already has a skill loader that reads `SKILL.md` files from a directory and
//! surfaces them by their own triggers, at the moment they apply. A blob
//! returned once is read once; an installed skill keeps working for the rest of
//! the session and every session after it. So this module's job is to report
//! **whether each skill is installed** and hand over the one action that
//! installs it - a concrete gap the agent can close, rather than advice.
//!
//! ## Three origins, and why they are handled differently
//!
//! `embedded` skills are DevFive's own but live in another repository -
//! devup-ui, vespera, vespertide. Their canonical `SKILL.md` is vendored into
//! the binary as an offline fallback; install prefers current upstream documents.
//! The fallback matters because the
//! situation this exists for, a bare machine, is the one in which a download is
//! least likely to work. A vendored copy can fall behind its repository, so
//! each one pins the commit it copied and `scripts/refresh-skills.mjs` is how
//! that copy is moved forward.
//!
//! `own` skills are written here, in this repository - devfive-frontend. There
//! is no upstream to vendor from and nothing for the refresh script to compare
//! against, so they pin no commit; this repository's own history is their
//! provenance. They are otherwise installed exactly like an embedded skill,
//! which is the point of giving them an origin rather than a special case.
//!
//! `external` skills belong to someone else. vercel-labs/agent-skills publishes
//! **no LICENSE file**, so its content is all-rights-reserved and devup-mcp
//! neither vendors nor redistributes it. They are also multi-file - one of them
//! is a `SKILL.md` plus an `AGENTS.md` and some seventy rule files - so copying
//! them was never the right shape anyway. devup-mcp reports where they come
//! from and the command their publisher provides, and the agent runs it.
//!
//! devup-mcp never executes that command. A design-to-code server that shells
//! out to a package installer is both out of character and a way to turn one
//! compromised registry entry into arbitrary execution on every machine that
//! ever exported a screen.

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
pub const MANIFEST_JSON: &str = include_str!("skills/manifest.json");

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
        "devup-ui",
        &[("SKILL.md", include_str!("skills/devup-ui/SKILL.md"))],
    ),
    (
        "devfive-frontend",
        &[
            ("SKILL.md", include_str!("skills/devfive-frontend/SKILL.md")),
            (
                "references/critical-rules.md",
                include_str!("skills/devfive-frontend/references/critical-rules.md"),
            ),
            (
                "references/anti-patterns.md",
                include_str!("skills/devfive-frontend/references/anti-patterns.md"),
            ),
            (
                "references/common-patterns.md",
                include_str!("skills/devfive-frontend/references/common-patterns.md"),
            ),
            (
                "references/theme-colors.md",
                include_str!("skills/devfive-frontend/references/theme-colors.md"),
            ),
        ],
    ),
    (
        "changepacks",
        &[("SKILL.md", include_str!("skills/changepacks/SKILL.md"))],
    ),
    (
        "vespera",
        &[("SKILL.md", include_str!("skills/vespera/SKILL.md"))],
    ),
    (
        "vespertide",
        &[("SKILL.md", include_str!("skills/vespertide/SKILL.md"))],
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
    pub fn new(project_root: Option<&str>, fallback: &Path, runtime: Runtime) -> Self {
        Self {
            project: project_root.map_or_else(|| fallback.to_path_buf(), PathBuf::from),
            home: home(),
            runtime,
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

impl Skill {
    /// The manifest's path is either the entry file or the skill directory.
    /// References live beside that entry, even for nested upstream skills.
    fn upstream_url(&self, relative: &str) -> String {
        let path = self.record.path.trim_end_matches('/');
        let directory = if path == ENTRY_DOCUMENT {
            ""
        } else {
            path.strip_suffix("/SKILL.md").unwrap_or(path)
        };
        let document = if directory.is_empty() {
            relative.to_owned()
        } else {
            format!("{directory}/{relative}")
        };
        format!(
            "https://raw.githubusercontent.com/{}/HEAD/{document}",
            self.record.repo
        )
    }

    /// Resolve the complete set before staging any of it. One failed reference
    /// must not leave a new entry document next to old embedded references.
    async fn fetch_documents(
        &self,
        upstream: &dyn SkillUpstream,
        deadline: tokio::time::Instant,
    ) -> Result<FetchedDocuments, (String, SkillFetchError)> {
        let mut documents = Vec::new();
        let mut provenance = Vec::new();
        for (relative, _) in self.texts.expect("only carried skills can be fetched") {
            let url = self.upstream_url(relative);
            let timeout_error = || {
                SkillFetchError::Network(
                    "the four-second skill install fetch budget elapsed".to_owned(),
                )
            };
            if tokio::time::Instant::now() >= deadline {
                return Err((url, timeout_error()));
            }
            let fetched = tokio::time::timeout_at(deadline, upstream.fetch(&url))
                .await
                .unwrap_or_else(|_| Err(timeout_error()))
                .map_err(|error| (url.clone(), error))?;
            let text = std::str::from_utf8(&fetched.contents).map_err(|error| {
                (
                    url.clone(),
                    SkillFetchError::Network(format!("upstream document is not UTF-8: {error}")),
                )
            })?;
            let record = SkillProvenance {
                source_url: url,
                etag: fetched.etag,
                fetched_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                sha256: crate::skills::sha256(&fetched.contents),
            };
            let contents = if *relative == ENTRY_DOCUMENT {
                self.annotated_with(text, Some(&record))
            } else {
                text.to_owned()
            };
            documents.push((*relative, contents));
            provenance.push(serde_json::json!({"path": relative, "provenance": record}));
        }
        Ok(FetchedDocuments {
            documents,
            provenance,
        })
    }

    /// The provenance a reader needs alongside the text: which revision this
    /// is, and where the current one lives. Carried in the body rather than
    /// returned beside it, because the installed file outlives this response
    /// and a reader who finds rules on disk with no revision cannot tell
    /// whether to trust them over the repository.
    ///
    /// Placed **after** the YAML frontmatter, never before it. A `SKILL.md`
    /// begins with `---` and every skill loader reads that delimiter at byte
    /// zero; a comment in front of it makes the frontmatter unparseable, and an
    /// installed skill that does not load is worse than no install at all -
    /// it looks done. This is the whole feature's failure mode, so the offset
    /// is computed rather than assumed, and falls back to prepending only for a
    /// document that has no frontmatter to protect.
    ///
    /// `None` for external skills, which have no text here to carry.
    pub fn document(&self) -> Option<String> {
        Some(self.annotated(self.entry_text()?))
    }

    /// The entry document's text exactly as it sits in the binary, with no
    /// provenance note added. `None` for external skills.
    pub fn entry_text(&self) -> Option<&'static str> {
        self.texts?
            .iter()
            .find(|(path, _)| *path == ENTRY_DOCUMENT)
            .map(|(_, text)| *text)
    }

    /// Every file an install writes, as `(path relative to the skill
    /// directory, contents)`.
    ///
    /// Only the entry document is annotated. A reference file is written byte
    /// for byte, which keeps the digest in the manifest true of the installed
    /// file as well - the note is worth breaking that for on the one document a
    /// reader opens cold, and not on the four it links to.
    pub fn installable_documents(&self) -> Option<Vec<(&'static str, String)>> {
        Some(
            self.texts?
                .iter()
                .map(|(path, text)| {
                    let contents = if *path == ENTRY_DOCUMENT {
                        self.annotated(text)
                    } else {
                        (*text).to_owned()
                    };
                    (*path, contents)
                })
                .collect(),
        )
    }

    /// The document with its provenance note placed after any frontmatter.
    fn annotated(&self, text: &str) -> String {
        self.annotated_with(text, None)
    }

    /// How the copy at `entry` compares with the one this binary would write.
    ///
    /// Byte comparison against exactly what an install produces, rather than
    /// against the manifest digest: the entry document is annotated on the way
    /// to disk, so its installed bytes were never the digest's bytes and a
    /// digest check would call every correct install stale.
    ///
    /// A fetched copy is read as current without comparing anything. It came
    /// from upstream HEAD, so it is at least as new as the copy vendored into
    /// this binary, and deciding otherwise would need the network - which is
    /// exactly what the machine this matters on does not have.
    ///
    /// References are checked only when the entry document is current. When it
    /// is not, the references belong to whichever revision wrote the entry and
    /// reporting them as a separate fault would name a symptom instead of the
    /// cause.
    pub fn freshness_at(&self, entry: &Path) -> Freshness {
        let Some(expected) = self.entry_text() else {
            // Nothing of ours to compare against; devup-mcp never wrote it.
            return Freshness::Foreign;
        };
        let Ok(found) = std::fs::read_to_string(entry) else {
            return Freshness::Foreign;
        };
        if found != self.annotated(expected) {
            return if found.contains(FETCHED_MARKER) {
                Freshness::Current
            } else if found.contains(VENDORED_MARKER) || found.contains(AUTHORED_MARKER) {
                Freshness::Older
            } else {
                Freshness::Foreign
            };
        }
        let Some(directory) = entry.parent() else {
            return Freshness::Current;
        };
        for document in &self.record.documents {
            if document.path == ENTRY_DOCUMENT {
                continue;
            }
            let path = document
                .path
                .split('/')
                .fold(directory.to_path_buf(), |p, part| p.join(part));
            let matches = std::fs::read(&path)
                .is_ok_and(|bytes| crate::skills::sha256(&bytes) == document.sha256);
            if !matches {
                return Freshness::Incomplete;
            }
        }
        Freshness::Current
    }

    fn annotated_with(&self, text: &str, fetched: Option<&SkillProvenance>) -> String {
        let note = self.provenance_note(fetched);
        match frontmatter_end(text) {
            Some(end) => format!("{}\n{note}\n{}", &text[..end], &text[end..]),
            None => format!("{note}\n\n{text}"),
        }
    }

    /// What a reader who finds this file on disk needs in order to decide
    /// whether to trust it over the repository.
    ///
    /// An `own` skill gets a different note, not a filled-in version of the
    /// vendored one: it has no upstream commit, and printing "at unknown" would
    /// read as a lost revision rather than as one that never existed.
    fn provenance_note(&self, fetched: Option<&SkillProvenance>) -> String {
        let r = &self.record;
        if let Some(p) = fetched {
            return format!(
                "{}{} at {} (Unix seconds).\n     ETag: {}\n     SHA-256 of upstream bytes before annotation: {}\n     Why this matters here: {} -->",
                FETCHED_MARKER,
                p.source_url,
                p.fetched_at,
                p.etag.replace("--", "&#45;&#45;"),
                p.sha256,
                r.used_for,
            );
        }
        if r.origin == Origin::Own {
            return format!(
                "{marker}{repo}, at {path}.\n     \
                 Part of this devup-mcp build - there is no separate upstream for it to fall \
                 behind.\n     \
                 Current revision: {latest}\n     \
                 Why this matters here: {used} -->",
                marker = AUTHORED_MARKER,
                repo = r.repo,
                path = r.path,
                latest = r.latest_url,
                used = r.used_for,
            );
        }
        format!(
            "{marker}{repo}/{path} at {commit} ({at}).\n     \
             Embedded in this devup-mcp build; no network was used to read it.\n     \
             This revision: {source}\n     \
             Newer revisions, if any: {latest}\n     \
             Why this matters here: {used} -->",
            marker = VENDORED_MARKER,
            repo = r.repo,
            path = r.path,
            commit = r.commit.as_deref().unwrap_or("unknown"),
            at = r.committed_at.as_deref().unwrap_or("unknown"),
            source = r.source_url,
            latest = r.latest_url,
            used = r.used_for,
        )
    }

    /// The one action that closes the gap, in the imperative, with everything
    /// needed to carry it out.
    /// The freshness of the worst copy this runtime would load, or `None` when
    /// there is no copy at all.
    ///
    /// The worst one decides. A machine with a current copy in one root and a
    /// stale copy in another is loading both, and the stale one is the one
    /// that will be wrong.
    pub fn installed_freshness(&self, lookup: &Lookup) -> Option<Freshness> {
        lookup
            .installed_paths(&self.record.name)
            .iter()
            .map(|path| self.freshness_at(path))
            .max_by_key(|state| match state {
                Freshness::Current => 0,
                Freshness::Older => 1,
                Freshness::Incomplete => 2,
                Freshness::Foreign => 3,
            })
    }

    pub fn install_action(&self, lookup: &Lookup) -> serde_json::Value {
        let installed = lookup.installed_paths(&self.record.name);
        if !installed.is_empty() {
            let paths = installed
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            let revision = self
                .installed_freshness(lookup)
                .unwrap_or(Freshness::Current);
            if revision.should_refresh() {
                return serde_json::json!({
                    "installed": true,
                    "paths": paths,
                    "revision": revision.as_str(),
                    "action": "devup_skills",
                    "arguments": {"action": "install", "names": [self.record.name]},
                    "why": if revision == Freshness::Incomplete {
                        "The SKILL.md on disk is this build's, but a document it links to is \
                         missing or has different bytes. It looks installed and its links go \
                         nowhere."
                    } else {
                        "The copy on disk was written by a different devup-mcp build. It still \
                         loads, and it describes conventions this binary no longer emits."
                    },
                    "how": "Call devup_skills with action \"install\". A copy devup-mcp wrote is \
                            rewritten in place; nothing else is touched.",
                });
            }
            return serde_json::json!({
                "installed": true,
                "paths": paths,
                "revision": revision.as_str(),
                "action": null,
                "how": if revision == Freshness::Foreign {
                    "Installed, but this document is not one devup-mcp wrote - no provenance \
                     note. It is left exactly as it is, because replacing it would destroy work \
                     this server did not do. Delete it first if you want devup-mcp's copy."
                } else {
                    "Already installed and current. Your skill loader picks it up by its own \
                     triggers; nothing to do."
                },
            });
        }
        match self.record.origin {
            Origin::Embedded | Origin::Own => {
                let roots = lookup.existing_roots();
                let targets = lookup.target_roots();
                let writes = targets
                    .iter()
                    .flat_map(|target| {
                        self.record.documents.iter().map(move |document| {
                            document_path(target, &self.record.name, &document.path)
                                .display()
                                .to_string()
                        })
                    })
                    .collect::<Vec<_>>();
                serde_json::json!({
                    "installed": false,
                    "paths": [],
                    "action": "devup_skills",
                    "arguments": {"action": "install", "names": [self.record.name]},
                    "writesTo": writes,
                    "how": "Call devup_skills with action \"install\". Embedded skills prefer current upstream documents with offline fallback; own skills use the binary. Then load it the way your runtime loads \
                            a project skill.",
                    "runtime": lookup.runtime.as_str(),
                    "rootChoice": if !roots.is_empty() {
                        format!("Using the existing root {}.", targets[0].display())
                    } else if lookup.runtime == Runtime::Unknown {
                        "No skill root exists yet and the MCP client did not identify itself, \
                         so every known convention is written. One of them is the one your \
                         runtime reads."
                            .to_owned()
                    } else {
                        format!(
                            "No skill root exists yet, so {} is created - the convention {} \
                             loads from.",
                            targets[0].display(),
                            lookup.runtime.as_str()
                        )
                    },
                })
            }
            Origin::External => serde_json::json!({
                "installed": false,
                "paths": [],
                "action": "run-this-yourself",
                "command": self.record.install_command,
                "how": "devup-mcp does not vendor or run this. Run the command yourself, then load \
                        the skill the way your runtime loads an installed skill.",
                "whyNotBundled": self.record.license_note,
                "source": self.record.source_url,
            }),
        }
    }
}

/// What every caller of `devup_skills` gets back: the gap, per skill.
///
/// The shape is deliberately the same for `status` and after `install`, so the
/// second call is how the agent confirms the first one worked rather than
/// something it has to take on trust.
pub fn report(lookup: &Lookup) -> serde_json::Value {
    let entries = all()
        .iter()
        .map(|skill| {
            let r = &skill.record;
            let mut entry = serde_json::json!({
                "name": r.name,
                "origin": r.origin.as_str(),
                "title": r.title,
                "description": r.description,
                "whyYouNeedIt": r.used_for,
                "source": r.source_url,
                "latest": r.latest_url,
            });
            if r.origin.is_carried() {
                // Every file, not just a count: a reader deciding whether the
                // copy on disk is the one this binary carries needs the digest
                // of the document they are looking at, and for a multi-document
                // skill that is not the entry document.
                entry["documents"] = serde_json::json!(
                    r.documents
                        .iter()
                        .map(|document| serde_json::json!({
                            "path": document.path,
                            "bytes": document.bytes,
                            "sha256": document.sha256,
                        }))
                        .collect::<Vec<_>>()
                );
                entry["offlineRead"] = serde_json::json!(skill.uri);
            }
            if let Some(commit) = &r.commit {
                entry["embeddedRevision"] = serde_json::json!({
                    "commit": commit,
                    "committedAt": r.committed_at,
                    "note": "The revision inside this binary. It does not move when the \
                             repository does; `latest` is where a newer one would be.",
                });
            } else if r.origin == Origin::Own {
                entry["embeddedRevision"] = serde_json::json!({
                    "commit": serde_json::Value::Null,
                    "note": "Authored in devup-mcp itself, so there is no upstream revision to \
                             pin and nothing for it to fall behind. It moves when this binary \
                             does.",
                });
            }
            if r.origin == Origin::External {
                entry["license"] = serde_json::json!(r.license);
            }
            let action = skill.install_action(lookup);
            entry["installed"] = action["installed"].clone();
            entry["installState"] = action;
            entry
        })
        .collect::<Vec<_>>();
    let missing = entries
        .iter()
        .filter(|entry| entry["installed"] == false)
        .count();
    // Counted apart from `missing`, because the two need different words from
    // the caller: one is a skill the agent has never seen, the other is one it
    // is reading right now and getting outdated rules from.
    let stale = entries
        .iter()
        .filter(|entry| {
            entry["installState"]["action"] == "devup_skills" && entry["installed"] == true
        })
        .map(|entry| entry["name"].clone())
        .collect::<Vec<_>>();
    let mut report = serde_json::json!({
        "workspace": lookup.project.display().to_string(),
        "runtime": {
            "detected": lookup.runtime.as_str(),
            "from": "The MCP clientInfo name sent at initialize.",
            "why": "Runtimes do not read the same directory. Installing into one this runtime \
                    never opens reports success and still never loads, which is the failure this \
                    field exists to make visible.",
        },
        "skillRoots": {
            "known": lookup.runtime.project_roots(),
            "existing": lookup.existing_roots()
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
            "wouldUse": lookup.target_roots()
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
            "knownUser": lookup.runtime.user_roots(),
            "existingUser": lookup.user_roots()
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
            "userRootsNote": "Machine-wide roots are read to decide `installed`, never written: \
                              devup-mcp writes only inside an allowed output root. A skill found \
                              in one of these is already loaded by its runtime and needs nothing.",
            "writeConventions": lookup.runtime.write_roots(),
            "allKnownRoots": EVERY_PROJECT_ROOT,
            "scopeNote": "`known` is every root this runtime *reads*, so an install someone else \
                          made is found rather than duplicated. `writeConventions` is the \
                          narrower list an install may create, and `wouldUse` is what this call \
                          would actually write. For an unidentified client the first is every \
                          convention and the second is the three broad ones, because writing a \
                          copy into each of a dozen editor directories is worse than the problem \
                          it solves.",
        },
        "installedCount": entries.len() - missing,
        "missingCount": missing,
        "outdated": stale,
        "outdatedNote": "Installed, loading, and not what this build writes. `installed` means the \
                         file is there; `installState.revision` says whether it is current, and a \
                         skill listed here is teaching an agent rules this binary no longer emits. \
                         Installing again rewrites devup-mcp's own copies and leaves a `foreign` \
                         one - a document with no devup-mcp provenance note - alone.",
        "skills": entries,
        "machineWide": machine_wide(lookup),
        "how": "These are the conventions for the code devup-mcp emits and reads. Install the \
                missing ones, then let your own skill loader surface them - an installed skill \
                keeps applying for every later session, which is the thing reading a document \
                once does not do.",
        "boundary": "devup-mcp installs only carried skills: embedded skills prefer current upstream documents with offline fallback, and own skills use the binary. External skills are never fetched or written; their install command is yours to run.",
    });
    if let Some(obligation) = changepacks_obligation(&lookup.project) {
        report["repoObligations"] = serde_json::json!({ "changepacks": obligation });
    }
    report
}

/// Where a once-per-machine install would go, and why devup-mcp does not do it.
///
/// Installing per project is correct and self-healing - every export names the
/// gap again - but it is one install per repository, and someone setting up a
/// machine usually wants one install for all of them. The home directory is
/// outside the allowed write root by design, and widening that so a
/// design-to-code server can write to `$HOME` is not a trade worth making for
/// convenience. So the path is handed over and the person runs it.
fn machine_wide(lookup: &Lookup) -> serde_json::Value {
    let Some(home) = &lookup.home else {
        return serde_json::json!({
            "available": false,
            "why": "No home directory is set for this process, so there is nowhere to name.",
        });
    };
    let directory = join_root(home, lookup.runtime.user_roots()[0]);
    serde_json::json!({
        "available": true,
        "directory": display_path(&directory),
        "appliesTo": "Every project on this machine, not just this one.",
        "action": "run-this-yourself",
        "why": "devup-mcp writes only inside an allowed output root, and the home directory is \
                not one. Widening that so a design-to-code server can write to $HOME is not a \
                trade worth making, so the path is yours to fill.",
        "how": "Copy the installed skill directories there, or install them with your runtime's \
                own skill tooling. A skill found in this directory is reported as installed by \
                every later call, for every project.",
        "readNotWritten": true,
    })
}

/// What a `.changepacks/` directory obliges a pull request in this workspace to
/// carry, or `None` when the repository does not use changepacks.
///
/// Reported here rather than left to the skill because the skill only helps a
/// caller who installed and loaded it, while this is in the response of a tool
/// `instructions` already tells every session to call. The failure being
/// addressed is a pull request that silently ships without a version bump, and
/// an agent that never asked about changepacks is exactly the one that produces
/// it.
///
/// Reads the config rather than assuming a rule: which paths are tracked is
/// decided by `ignore` with `!` negations, and it differs per repository.
fn changepacks_obligation(project: &Path) -> Option<serde_json::Value> {
    let directory = project.join(".changepacks");
    let config_path = directory.join("config.json");
    if !config_path.is_file() {
        return None;
    }
    let config: serde_json::Value = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(serde_json::Value::Null);

    // A pending log is one already written for an unreleased change. Its
    // presence answers "has someone on this branch done this already", which is
    // the question an agent about to add a second one needs answered.
    let pending = std::fs::read_dir(&directory)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| name.starts_with("changepack_log_") && name.ends_with(".json"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(serde_json::json!({
        "detected": display_path(&directory),
        "obligation": "A pull request that changes a tracked path must add a changepack log. \
                       Without one the version never moves, so the change reaches the base branch \
                       and is never released.",
        "createWith": "bunx @changepacks/cli --yes --update-type <major|minor|patch> --message \"<why this change exists>\"",
        "whyNotBare": "Running the tool with no flags opens an interactive selection UI, which \
                       hangs or is cancelled in a non-TTY shell. That is the usual reason a pull \
                       request arrives without the changepack it needed.",
        "tracks": config.get("ignore").cloned().unwrap_or(serde_json::Value::Null),
        "tracksNote": "Patterns from .changepacks/config.json. A leading `!` marks a tracked path; \
                       everything else is ignored. Whether your change needs a changepack is decided \
                       by this list, not by a general rule.",
        "baseBranch": config.get("baseBranch").cloned().unwrap_or(serde_json::Value::Null),
        "pendingLogs": pending,
        "skill": "changepacks",
    }))
}

/// A path as a reader would type it, with the separators their editor shows.
fn display_path(path: &Path) -> String {
    path.display().to_string()
}

/// Writes the documents of the carried skills that are missing.
///
/// Goes through the same [`OutputPolicy`] every other file this server writes
/// goes through, so a skill lands under an allowed write root or not at all,
/// and through one [`OutputTransaction`], so a partial install does not leave
/// half a set behind.
///
/// That last part is why a multi-document skill is staged whole rather than
/// document by document: a `SKILL.md` that survived next to four references
/// that did not is the failure this feature exists to avoid, because it looks
/// installed and its links go nowhere.
///
/// [`OutputPolicy`]: super::output::OutputPolicy
/// [`OutputTransaction`]: super::output::OutputTransaction
pub async fn install(
    policy: &super::output::OutputPolicy,
    lookup: &Lookup,
    requested: &[String],
) -> Result<serde_json::Value, devup_mcp_figma::DevupError> {
    install_with(policy, lookup, requested, &InstallUpstream).await
}

async fn install_with(
    policy: &super::output::OutputPolicy,
    lookup: &Lookup,
    requested: &[String],
    upstream: &dyn SkillUpstream,
) -> Result<serde_json::Value, devup_mcp_figma::DevupError> {
    use devup_mcp_figma::{DevupError, ErrorCode};

    let project = lookup.project.clone();
    if let Some(unknown) = requested.iter().find(|name| find_by_name(name).is_none()) {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            format!("{unknown} is not a skill devup-mcp knows about."),
            false,
            serde_json::json!({
                "known": all().iter().map(|s| &s.record.name).collect::<Vec<_>>(),
            }),
        ));
    }

    let wanted: Vec<&Skill> = all()
        .iter()
        .filter(|skill| requested.is_empty() || requested.contains(&skill.record.name))
        .collect();

    // Named explicitly or not, an external skill cannot be written from here.
    // Saying so per skill, rather than refusing the call, keeps a plain
    // `install` with no names working as "install everything you can".
    let external = wanted
        .iter()
        .filter(|skill| skill.record.origin == Origin::External)
        .map(|skill| {
            serde_json::json!({
                "name": skill.record.name,
                "command": skill.record.install_command,
                "why": skill.record.license_note,
                "source": skill.record.source_url,
            })
        })
        .collect::<Vec<_>>();

    let roots = lookup.target_roots();
    let mut transaction = super::output::OutputTransaction::new();
    let mut written = Vec::new();
    let mut already = Vec::new();
    let mut refreshed = Vec::new();
    let mut left_alone = Vec::new();
    let mut warnings = Vec::new();
    // One budget for the call, not four seconds multiplied by the registry.
    let deadline = tokio::time::Instant::now() + FETCH_TIMEOUT;
    for skill in wanted
        .iter()
        .filter(|skill| skill.record.origin.is_carried())
    {
        let name = &skill.record.name;
        // Presence alone used to end the decision here, which is why there was
        // no way to update a skill: the first install a machine ever did was
        // the last one it would get. What is on disk decides now.
        let present = lookup.installed_paths(name);
        if !present.is_empty() {
            let states = present
                .iter()
                .map(|path| (path.clone(), skill.freshness_at(path)))
                .collect::<Vec<_>>();
            let refreshable = states
                .iter()
                .filter(|(_, state)| state.should_refresh())
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            for (path, state) in &states {
                if *state == Freshness::Foreign {
                    left_alone.push(serde_json::json!({
                        "name": name,
                        "path": path.display().to_string(),
                        "revision": state.as_str(),
                        "why": "No devup-mcp provenance note, so devup-mcp did not write it and \
                                will not replace it. Delete it first to take devup-mcp's copy.",
                    }));
                }
            }
            if refreshable.is_empty() {
                already.push(name.clone());
                continue;
            }
            // Rewrite where the stale copies actually are, not where a fresh
            // install would have chosen - a copy left behind in a root the
            // chosen one is not in would keep being loaded and keep being old.
            refreshed.push(serde_json::json!({
                "name": name,
                "paths": refreshable.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            }));
            stage_documents(
                policy,
                &mut transaction,
                skill,
                upstream,
                deadline,
                &refreshable
                    .iter()
                    .filter_map(|path| Some(path.parent()?.parent()?.to_path_buf()))
                    .collect::<Vec<_>>(),
                &project,
                &mut warnings,
                &mut written,
            )
            .await?;
            continue;
        }
        stage_documents(
            policy,
            &mut transaction,
            skill,
            upstream,
            deadline,
            &roots,
            &project,
            &mut warnings,
            &mut written,
        )
        .await?;
    }
    transaction.commit()?;

    Ok(serde_json::json!({
        "installed": written,
        "refreshed": refreshed,
        "alreadyPresent": already,
        "leftAlone": left_alone,
        "notInstallable": external,
        "warnings": warnings,
        "roots": roots.iter().map(|root| root.display().to_string()).collect::<Vec<_>>(),
        "runtime": lookup.runtime.as_str(),
        "nextAction": if written.is_empty() && external.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::json!({
                "how": "Load the newly installed skills the way your runtime loads a project \
                        skill, and run any command under notInstallable yourself. Call \
                        devup_skills again to confirm the state changed.",
            })
        },
        "boundary": "Only carried skills were written, using current upstream documents where available or the embedded copy. External skills were not fetched or written, and no install command was executed.",
    }))
}

/// Resolves one skill's documents and stages every one of them under `roots`.
///
/// Shared by the first install and by a refresh so the two cannot drift: a
/// refresh that wrote a different set of files than an install would have is
/// how a skill ends up half one revision and half another.
///
/// The whole set is staged, never a document at a time, for the reason this
/// module exists: a `SKILL.md` that survived next to four references that did
/// not looks installed and its links go nowhere.
#[allow(clippy::too_many_arguments)]
async fn stage_documents(
    policy: &super::output::OutputPolicy,
    transaction: &mut super::output::OutputTransaction,
    skill: &Skill,
    upstream: &dyn SkillUpstream,
    deadline: tokio::time::Instant,
    roots: &[PathBuf],
    project: &Path,
    warnings: &mut Vec<serde_json::Value>,
    written: &mut Vec<serde_json::Value>,
) -> Result<(), devup_mcp_figma::DevupError> {
    use devup_mcp_figma::{DevupError, ErrorCode};

    let name = &skill.record.name;
    let mut documents = skill
        .installable_documents()
        .expect("a carried skill always has documents");
    let mut source = "embedded";
    let mut reason = Some("authored in this repository; no upstream".to_owned());
    let mut provenance = Vec::new();
    if skill.record.origin == Origin::Embedded {
        match skill.fetch_documents(upstream, deadline).await {
            Ok(fetched) => {
                documents = fetched.documents;
                provenance = fetched.provenance;
                source = "fetched";
                reason = None;
            }
            Err((url, error)) => {
                reason = Some(format!("{url}: {error}"));
                if error == SkillFetchError::NotFound {
                    warnings.push(serde_json::json!({
                        "name": name,
                        "sourceUrl": url,
                        "message": "Upstream returned 404; the document may have moved. Check the skill manifest. The complete embedded copy was installed.",
                    }));
                }
            }
        }
    }
    let mut paths = Vec::with_capacity(documents.len() * roots.len());
    for root in roots {
        for (relative, contents) in &documents {
            // The skill root has to sit inside an allowed write root. Said in
            // those words, because the caller who hits this passed a
            // `projectRoot`, not an `outputPath`, and being told an
            // `outputPath` is out of bounds names something they never sent.
            let target = policy
                .resolve(&document_path(root, name, relative).display().to_string())
                .map_err(|error| {
                    DevupError::with_details(
                        ErrorCode::DevupInvalidInput,
                        format!(
                            "Cannot install skills into {}: it is outside this server's allowed write roots.",
                            root.display()
                        ),
                        false,
                        serde_json::json!({
                            "projectRoot": project.display().to_string(),
                            "skillRoot": root.display().to_string(),
                            "underlying": error.message,
                            "how": "Start devup-mcp with --allow-write-root pointing at this project, \
                                    or omit projectRoot to install into the server's own write root.",
                        }),
                    )
                })?;
            paths.push(target.display_path().display().to_string());
            transaction.stage(
                format!("skill:{name}:{}:{relative}", root.display()),
                target,
                contents.as_bytes(),
            )?;
        }
    }
    written.push(
        serde_json::json!({"name": name, "paths": paths, "source": source, "reason": reason, "documents": provenance}),
    );
    Ok(())
}

#[cfg(test)]
#[path = "skills/fetch_tests.rs"]
mod fetch_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[tokio::test]
    async fn an_own_install_reports_the_binary_as_its_source() {
        let project = scratch("own-source");
        let policy = super::super::output::OutputPolicy::from_roots(vec![project.clone()]).unwrap();
        let report = install(
            &policy,
            &unknown_client(&project),
            &["devfive-frontend".to_owned()],
        )
        .await
        .unwrap();
        assert_eq!(report["installed"][0]["source"], "embedded");
        assert_eq!(
            report["installed"][0]["reason"],
            "authored in this repository; no upstream"
        );
        drop(policy);
        std::fs::remove_dir_all(project).unwrap();
    }

    /// A lookup for a client that never named itself, seeing no machine-wide
    /// roots.
    ///
    /// `project_only` is load-bearing: reading the real home would make these
    /// assertions depend on whether whoever runs them has devup-ui in
    /// `~/.claude/skills`, and most people working on this repository do.
    fn unknown_client(project: &Path) -> Lookup {
        Lookup::project_only(project.to_path_buf(), Runtime::Unknown)
    }

    fn scratch(label: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("devup-skills-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch directory");
        path
    }

    /// Each origin has its own obligations, and a record that mixes them will
    /// mislead. A carried entry must list every document it writes and match
    /// each one byte for byte; an embedded one must additionally pin the
    /// upstream revision it copied, an `own` one must not pretend to have a
    /// revision it cannot have, and an external one must carry no bytes at all
    /// and must say how to install it instead.
    #[test]
    fn every_origin_is_well_formed() {
        let manifest: Manifest = serde_json::from_str(MANIFEST_JSON).unwrap();
        let carried = manifest
            .skills
            .iter()
            .filter(|record| record.origin.is_carried())
            .count();
        assert_eq!(
            carried,
            EMBEDDED.len(),
            "manifest and embedded set disagree on how many skills are in the binary"
        );

        for record in &manifest.skills {
            if record.origin.is_carried() {
                let (_, documents) = EMBEDDED
                    .iter()
                    .find(|(name, _)| *name == record.name)
                    .unwrap_or_else(|| panic!("{} is carried in name only", record.name));
                assert_eq!(
                    documents.len(),
                    record.documents.len(),
                    "{}: manifest and binary disagree on how many documents this skill has",
                    record.name
                );
                assert!(
                    record.documents.iter().any(|d| d.path == ENTRY_DOCUMENT),
                    "{}: a skill with no {ENTRY_DOCUMENT} is one no loader opens",
                    record.name
                );
                for document in &record.documents {
                    let (_, text) = documents
                        .iter()
                        .find(|(path, _)| *path == document.path)
                        .unwrap_or_else(|| {
                            panic!("{}: {} is in the manifest only", record.name, document.path)
                        });
                    assert_eq!(
                        text.len(),
                        document.bytes,
                        "{}: {}: byte count",
                        record.name,
                        document.path
                    );
                    let digest: String = Sha256::digest(text.as_bytes())
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect();
                    assert_eq!(
                        digest, document.sha256,
                        "{}: {}: the document was edited without updating its record.",
                        record.name, document.path
                    );
                }
            }

            match record.origin {
                Origin::Embedded => {
                    let commit = record
                        .commit
                        .as_deref()
                        .expect("embedded entries pin the revision they copied");
                    assert!(
                        record.source_url.contains(commit),
                        "{}: sourceUrl does not pin {commit}",
                        record.name
                    );
                }
                Origin::Own => {
                    // Not an oversight to be filled in later: there is no second
                    // repository for this document to lag behind, and a pinned
                    // commit would invite the refresh script to try to move it.
                    assert!(
                        record.commit.is_none() && record.committed_at.is_none(),
                        "{}: an own skill has no upstream revision to pin",
                        record.name
                    );
                }
                Origin::External => {
                    assert!(
                        !EMBEDDED.iter().any(|(name, _)| *name == record.name),
                        "{}: external content must not be in the binary",
                        record.name
                    );
                    assert!(
                        record.documents.is_empty(),
                        "{}: an external entry claims embedded bytes",
                        record.name
                    );
                    assert!(
                        record
                            .install_command
                            .as_deref()
                            .is_some_and(|command| !command.trim().is_empty()),
                        "{}: external with no way to install it is a dead end",
                        record.name
                    );
                    assert!(
                        record.license_note.is_some(),
                        "{}: not bundling someone else's work has to say why",
                        record.name
                    );
                }
            }
        }
    }

    /// A multi-document skill installs whole or not at all, and every reference
    /// its `SKILL.md` links to has to be one of the files that install writes.
    /// A `SKILL.md` alone, with dead links where its rules were, is the shape
    /// that looks installed and is not.
    #[test]
    fn a_multi_document_skill_writes_every_file_it_links_to() {
        let skill = find_by_name("devfive-frontend").expect("devfive-frontend is registered");
        assert_eq!(skill.record.origin, Origin::Own);

        let documents = skill.installable_documents().expect("carried");
        assert!(
            documents.len() > 1,
            "this test is about the multi-document case"
        );

        let entry = skill.entry_text().expect("carried");
        for (path, _) in documents.iter().filter(|(path, _)| *path != ENTRY_DOCUMENT) {
            assert!(
                entry.contains(path),
                "{path} is installed but SKILL.md never links to it"
            );
        }

        // Only the entry document is annotated; the references stay byte-exact
        // so the manifest digest is still true of what lands on disk.
        for (path, contents) in &documents {
            let record = skill
                .record
                .documents
                .iter()
                .find(|d| d.path == *path)
                .expect("every installed document is recorded");
            if *path == ENTRY_DOCUMENT {
                assert!(contents.len() > record.bytes, "the entry carries its note");
            } else {
                assert_eq!(contents.len(), record.bytes, "{path} was rewritten");
            }
        }
    }

    /// The reported gap has to match the disk, in both directions, or the
    /// inducement is noise: a false "missing" makes the agent write a duplicate
    /// copy, and a false "installed" leaves it guessing devup-ui forever.
    #[test]
    fn install_state_follows_the_disk_across_every_known_root() {
        let project = scratch("state");
        let skill = find_by_name("devup-ui").expect("devup-ui is registered");

        let lookup = unknown_client(&project);

        let missing = skill.install_action(&lookup);
        assert_eq!(missing["installed"], false);
        assert_eq!(missing["action"], "devup_skills");

        // Installed by hand into the last root, not the preferred one.
        let root = join_root(&project, SKILL_ROOTS[SKILL_ROOTS.len() - 1]);
        let path = install_path(&root, "devup-ui");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# installed by hand").unwrap();

        let found = skill.install_action(&lookup);
        assert_eq!(
            found["installed"], true,
            "a hand-installed skill is installed"
        );
        assert_eq!(found["action"], serde_json::Value::Null);
        assert_eq!(found["paths"].as_array().unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&project);
    }

    /// The client names itself at `initialize`, and that name is the only
    /// thing that says which directory to write.
    #[test]
    fn the_runtime_is_read_from_the_client_name() {
        for (name, expected) in [
            ("codex", Runtime::Codex),
            ("Codex CLI", Runtime::Codex),
            ("claude-code", Runtime::ClaudeCode),
            ("Claude Code", Runtime::ClaudeCode),
            ("opencode", Runtime::Opencode),
            // `cursor-vscode` contains both names; Cursor has to win.
            ("cursor-vscode", Runtime::Cursor),
            ("Visual Studio Code", Runtime::VsCode),
            ("Code - OSS", Runtime::VsCode),
            ("Zed", Runtime::Zed),
            ("windsurf-client", Runtime::Windsurf),
            ("gemini-cli-mcp-client", Runtime::GeminiCli),
            ("@cline/core", Runtime::Cline),
            ("continue-cli-client", Runtime::Continue),
            ("amp-mcp-client", Runtime::Amp),
            ("goose", Runtime::Goose),
            // No fixed auto-discovered directory, so guessing one would be
            // worse than the fallback.
            ("JetBrains-IU-copilot-intellij", Runtime::Unknown),
            ("some-editor", Runtime::Unknown),
            // Short words must not be claimed out of unrelated names.
            ("analyzed-client", Runtime::Unknown),
            ("example-mcp", Runtime::Unknown),
        ] {
            assert_eq!(
                Runtime::from_client_name(name),
                expected,
                "{name} was read as the wrong runtime"
            );
        }
        // `opencode` contains `codex` backwards-of-nowhere, but it does
        // contain `code`; the ordering that actually matters is that the
        // substring test for opencode runs before the one for codex.
        assert_eq!(Runtime::from_client_name("opencode"), Runtime::Opencode);
    }

    /// The reported bug, as a test. A fresh project has no skill root at all,
    /// so the choice used to fall through to `SKILL_ROOTS[0]` -
    /// `.claude/skills` - which Codex never reads. The install reported
    /// success and the skill never loaded.
    #[test]
    fn a_fresh_project_installs_where_this_runtime_actually_reads() {
        let project = scratch("fresh");
        let skill = find_by_name("devup-ui").unwrap();

        let codex = Lookup::project_only(project.clone(), Runtime::Codex);
        let writes = skill.install_action(&codex)["writesTo"][0]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            writes.contains(".agents"),
            "Codex would be given {writes}, which it does not read"
        );
        assert!(!writes.contains(".claude"), "{writes}");

        let claude = Lookup::project_only(project.clone(), Runtime::ClaudeCode);
        let writes = skill.install_action(&claude)["writesTo"][0]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(writes.contains(".claude"), "{writes}");

        let _ = std::fs::remove_dir_all(&project);
    }

    /// With no client name and no existing root there is nothing to choose on,
    /// and picking one of three is a two-in-three chance of writing where
    /// nothing looks.
    #[test]
    fn an_unnamed_client_is_given_every_convention() {
        let project = scratch("unnamed");
        let roots = Lookup::project_only(project.clone(), Runtime::Unknown).target_roots();
        assert_eq!(roots.len(), SKILL_ROOTS.len());
        for convention in SKILL_ROOTS {
            assert!(
                roots
                    .iter()
                    .any(|root| root == &join_root(&project, convention)),
                "{convention} would not be written, so a runtime reading it gets nothing"
            );
        }

        let _ = std::fs::remove_dir_all(&project);
    }

    /// The symmetric mistake, and the one that is silent: a file in a
    /// directory this runtime never opens is not loaded, and counting it says
    /// "installed" about a skill the agent will never see.
    #[test]
    fn a_skill_in_a_root_this_runtime_never_reads_is_not_installed() {
        let project = scratch("wrong-root");
        let path = install_path(&join_root(&project, ".claude/skills"), "devup-ui");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# in the wrong place for Codex").unwrap();

        assert!(
            Lookup::project_only(project.clone(), Runtime::Codex)
                .installed_paths("devup-ui")
                .is_empty(),
            "Codex was told a skill it cannot load is installed"
        );
        assert!(
            !Lookup::project_only(project.clone(), Runtime::ClaudeCode)
                .installed_paths("devup-ui")
                .is_empty(),
            "Claude Code reads exactly this directory"
        );

        let _ = std::fs::remove_dir_all(&project);
    }

    /// Most people install skills once for the machine, not per repository.
    /// Reporting those missing is what had a workspace with devup-ui in four
    /// loaded directories answering `installedCount: 0`.
    #[test]
    fn a_machine_wide_install_counts_as_installed() {
        let home = scratch("home");
        let project = scratch("home-project");
        let path = install_path(&join_root(&home, ".codex/skills"), "devup-ui");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# installed for the machine").unwrap();

        let lookup = Lookup {
            project: project.clone(),
            home: Some(home.clone()),
            runtime: Runtime::Codex,
        };
        assert_eq!(
            lookup.installed_paths("devup-ui"),
            vec![path],
            "a skill Codex loads from the home directory was reported missing"
        );
        let action = find_by_name("devup-ui").unwrap().install_action(&lookup);
        assert_eq!(action["installed"], true);
        assert_eq!(action["action"], serde_json::Value::Null, "nothing to do");

        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&project);
    }

    /// Writes what an install would write, so the freshness check has
    /// something honest to compare against.
    fn install_by_hand(project: &Path, runtime: Runtime, name: &str) -> PathBuf {
        let skill = find_by_name(name).unwrap();
        let root = join_root(project, runtime.write_roots()[0]);
        for (relative, contents) in skill.installable_documents().unwrap() {
            let path = document_path(&root, name, relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
        }
        install_path(&root, name)
    }

    /// `installed` used to mean `is_file()`, so a one-line placeholder read
    /// exactly like the current document. These are the four states that were
    /// all reported as one.
    #[test]
    fn freshness_separates_current_older_foreign_and_incomplete() {
        let project = scratch("freshness");
        let skill = find_by_name("devfive-frontend").unwrap();

        let entry = install_by_hand(&project, Runtime::Codex, "devfive-frontend");
        assert_eq!(
            skill.freshness_at(&entry),
            Freshness::Current,
            "what an install just wrote is not current"
        );

        // A reference gone: the shape that looks installed and whose links go
        // nowhere.
        let reference = entry
            .parent()
            .unwrap()
            .join("references")
            .join("anti-patterns.md");
        assert!(
            reference.is_file(),
            "this test needs the multi-document skill"
        );
        std::fs::remove_file(&reference).unwrap();
        assert_eq!(skill.freshness_at(&entry), Freshness::Incomplete);
        let _ = std::fs::remove_dir_all(&project);

        // Our provenance note, someone else's body: a different devup-mcp.
        let project = scratch("freshness-older");
        let entry = install_by_hand(&project, Runtime::Codex, "devfive-frontend");
        let body = std::fs::read_to_string(&entry).unwrap();
        std::fs::write(
            &entry,
            body.replace("Server Components", "Rules from an older build"),
        )
        .unwrap();
        assert_eq!(skill.freshness_at(&entry), Freshness::Older);

        // No note at all: not ours, and not ours to replace.
        std::fs::write(&entry, "# my own notes about this project\n").unwrap();
        assert_eq!(skill.freshness_at(&entry), Freshness::Foreign);

        let _ = std::fs::remove_dir_all(&project);
    }

    /// The practical sting of `is_file()`: `install` skipped anything already
    /// present, so the first install a machine did was the last one it got.
    #[tokio::test]
    async fn install_refreshes_an_older_copy_and_never_touches_a_foreign_one() {
        let project = scratch("refresh");
        let policy = super::super::output::OutputPolicy::from_roots(vec![project.clone()]).unwrap();
        let lookup = Lookup::project_only(project.clone(), Runtime::Codex);

        let entry = install_by_hand(&project, Runtime::Codex, "devfive-frontend");
        let current = std::fs::read_to_string(&entry).unwrap();
        std::fs::write(&entry, current.replace("Server Components", "stale")).unwrap();

        let report = install(&policy, &lookup, &["devfive-frontend".to_owned()])
            .await
            .unwrap();
        assert_eq!(
            report["refreshed"][0]["name"], "devfive-frontend",
            "an outdated copy was skipped instead of rewritten: {report}"
        );
        assert_eq!(
            std::fs::read_to_string(&entry).unwrap(),
            current,
            "the refresh did not restore this build's document"
        );

        // Hand-written: reported, never overwritten.
        let mine = "# my own notes\n";
        std::fs::write(&entry, mine).unwrap();
        let report = install(&policy, &lookup, &["devfive-frontend".to_owned()])
            .await
            .unwrap();
        assert_eq!(
            report["leftAlone"][0]["name"], "devfive-frontend",
            "{report}"
        );
        assert_eq!(
            std::fs::read_to_string(&entry).unwrap(),
            mine,
            "a document devup-mcp did not write was replaced"
        );

        drop(policy);
        let _ = std::fs::remove_dir_all(&project);
    }

    /// A skill someone installed once for the machine, in their editor's own
    /// home directory, was being reported missing - and the fix for "missing"
    /// is to write a second copy.
    #[test]
    fn every_runtime_finds_its_own_home_directory() {
        for (runtime, root) in [
            (Runtime::Cursor, ".cursor/skills"),
            (Runtime::GeminiCli, ".gemini/skills"),
            (Runtime::Cline, ".cline/skills"),
            (Runtime::Windsurf, ".codeium/windsurf/skills"),
            (Runtime::VsCode, ".copilot/skills"),
        ] {
            assert!(
                runtime.user_roots().contains(&root),
                "{} does not look in {root}, where its users install",
                runtime.as_str()
            );
            assert!(
                USER_SKILL_ROOTS.contains(&root),
                "an unidentified client would miss {root}"
            );
        }
    }

    /// Cline is why the table is worth having: it reads `.agents/skills` only
    /// from the home directory, so the shared project convention is one
    /// directory it never opens.
    #[test]
    fn a_client_that_does_not_share_the_common_convention_is_not_given_it() {
        assert!(!Runtime::Cline.project_roots().contains(&".agents/skills"));
        assert!(Runtime::Cline.user_roots().contains(&".agents/skills"));
        assert_eq!(Runtime::Cline.write_roots()[0], ".cline/skills");
    }

    /// An unidentified client reads everything and writes the three broad
    /// conventions. Reading narrowly would duplicate someone's install;
    /// writing broadly would litter a dozen editor directories.
    #[test]
    fn an_unknown_client_reads_wide_and_writes_narrow() {
        assert_eq!(Runtime::Unknown.project_roots(), EVERY_PROJECT_ROOT);
        assert_eq!(Runtime::Unknown.write_roots(), SKILL_ROOTS);
        assert!(Runtime::Unknown.project_roots().len() > SKILL_ROOTS.len());
        for convention in SKILL_ROOTS {
            assert!(EVERY_PROJECT_ROOT.contains(convention));
        }
    }

    /// An external skill is never answered with a devup-mcp call, because there
    /// is nothing here to install. It has to hand over its publisher's command
    /// and say why the bytes are not ours to ship.
    #[test]
    fn an_external_skill_hands_over_the_command_and_never_a_local_write() {
        let project = scratch("external");
        let skill = find_by_name("vercel-react-best-practices").expect("registered");
        assert!(
            skill.texts.is_none(),
            "external content is not in the binary"
        );
        assert!(skill.document().is_none());
        assert!(skill.installable_documents().is_none());

        let action = skill.install_action(&unknown_client(&project));
        assert_eq!(action["action"], "run-this-yourself");
        assert_eq!(action["command"], "npx skills add vercel-labs/agent-skills");
        assert!(
            action["writesTo"].is_null(),
            "devup-mcp writes nothing for it"
        );
        assert!(
            action["whyNotBundled"]
                .as_str()
                .unwrap()
                .contains("no LICENSE")
        );

        let _ = std::fs::remove_dir_all(&project);
    }

    /// An existing root is the project answering which runtime it is for.
    /// Guessing differently installs into a directory nothing reads.
    #[test]
    fn an_existing_skill_root_is_preferred_over_the_default() {
        let project = scratch("root");
        let chosen = join_root(&project, ".opencode/skill");
        std::fs::create_dir_all(&chosen).unwrap();

        let skill = find_by_name("devup-ui").unwrap();
        let action = skill.install_action(&unknown_client(&project));
        let writes_to = action["writesTo"][0].as_str().unwrap().to_owned();
        assert!(
            writes_to.contains(".opencode"),
            "existing root ignored; would write to {writes_to}"
        );

        let _ = std::fs::remove_dir_all(&project);
    }

    /// devup-mcp emits devup-ui TSX, so that is the skill whose absence produced
    /// the wrong code this module exists to stop.
    #[test]
    fn devup_ui_is_embedded_addressable_and_carries_its_age() {
        let skill = find_by_uri("devup://skill/devup-ui").expect("addressable");
        assert_eq!(skill.resource_name, "devup-skill-devup-ui");
        let text = skill.entry_text().expect("embedded");
        assert!(text.contains("Cannot run on the runtime"));

        let document = skill
            .document()
            .expect("embedded documents carry provenance");
        assert!(document.contains(skill.record.commit.as_deref().unwrap()));
    }

    /// The failure this feature cannot survive: an installed skill that does
    /// not load.
    ///
    /// A `SKILL.md` opens with `---` and every loader reads that delimiter at
    /// byte zero. Putting the provenance comment in front of it leaves a file
    /// that looks installed, is listed as installed, and is silently never
    /// applied - which is worse than not installing it, because nothing shows
    /// up to say so.
    #[test]
    fn an_installed_document_still_opens_with_its_frontmatter() {
        for skill in all().iter().filter(|skill| skill.entry_text().is_some()) {
            let document = skill.document().expect("carried");
            assert!(
                document.starts_with("---\n") || document.starts_with("---\r\n"),
                "{}: frontmatter no longer opens the file",
                skill.record.name
            );
            // The note landed inside the body, and the original frontmatter
            // keys are still inside the block rather than pushed out of it.
            let end = frontmatter_end(&document).expect("the block still closes");
            let head = &document[..end];
            assert!(head.contains("name:"), "{}: {head}", skill.record.name);
            // Whichever note this origin gets, it belongs after the block.
            // The repository comes from the record rather than a literal org:
            // a carried skill does not have to be one of ours, and hardcoding
            // `dev-five-git/` failed the first skill vendored from elsewhere.
            let opener = if skill.record.origin == Origin::Own {
                format!("Authored in {}", skill.record.repo)
            } else {
                format!("Vendored from {}", skill.record.repo)
            };
            let opener = opener.as_str();
            assert!(
                !head.contains(opener),
                "{}: the note must sit outside the frontmatter block",
                skill.record.name
            );
            assert!(
                document.contains(opener),
                "{}: no provenance note for origin {}",
                skill.record.name,
                skill.record.origin.as_str()
            );
            // Nothing of the original was dropped on the way through. The note
            // splits it, so the two halves are checked rather than the whole.
            let text = skill.entry_text().unwrap();
            let split = frontmatter_end(text).expect("a carried skill has frontmatter");
            assert!(document.contains(&text[..split]), "{}", skill.record.name);
            assert!(document.ends_with(&text[split..]), "{}", skill.record.name);
        }
    }

    /// A document with no frontmatter has nothing to protect, so the note goes
    /// where a reader sees it first.
    #[test]
    fn a_document_without_frontmatter_is_not_mistaken_for_one() {
        assert_eq!(frontmatter_end("# Plain\n\nbody\n"), None);
        assert_eq!(frontmatter_end("---\nname: x\n---\nbody\n"), Some(16));
        // A horizontal rule further down does not close a block that never
        // opened.
        assert_eq!(frontmatter_end("intro\n\n---\n\nmore\n"), None);
    }

    /// The obligation has to reach a caller who never asked about changepacks,
    /// because that caller is the one who opens the pull request without one.
    /// It also has to stay absent everywhere else: a repository that does not
    /// use changepacks must not be told to run it.
    #[test]
    fn a_changepacks_directory_is_reported_as_an_obligation() {
        let project = scratch("changepacks");

        assert!(
            report(&unknown_client(&project))["repoObligations"].is_null(),
            "a workspace with no .changepacks must carry no obligation"
        );

        let directory = project.join(".changepacks");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("config.json"),
            r#"{"ignore":["**","!/crates/*/Cargo.toml"],"baseBranch":"main"}"#,
        )
        .unwrap();
        std::fs::write(directory.join("changepack_log_existing.json"), "{}").unwrap();
        // Not a changepack log; it must not be counted as one.
        std::fs::write(directory.join("publish.tgz"), "").unwrap();

        let found = report(&unknown_client(&project))["repoObligations"]["changepacks"].clone();
        assert!(!found.is_null(), "the directory was not detected");

        // The command has to be the non-interactive one. Bare `changepacks`
        // opens a selection UI that hangs in the shells this runs in, which is
        // the whole reason the step gets skipped.
        let command = found["createWith"].as_str().unwrap();
        assert!(command.contains("--yes"), "{command}");
        assert!(command.contains("--update-type"), "{command}");
        assert!(command.contains("--message"), "{command}");

        // Read from the config, never assumed: which paths are tracked differs
        // per repository and deciding it here would be a guess.
        assert_eq!(found["tracks"][1], "!/crates/*/Cargo.toml");
        assert_eq!(found["baseBranch"], "main");

        let pending = found["pendingLogs"].as_array().unwrap();
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0], "changepack_log_existing.json");

        let _ = std::fs::remove_dir_all(&project);
    }
}
