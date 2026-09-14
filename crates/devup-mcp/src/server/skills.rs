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
//! the binary, so installing them needs no network. That matters because the
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

use serde::Deserialize;

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
            (
                "SKILL.md",
                include_str!("skills/devfive-frontend/SKILL.md"),
            ),
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

/// Which skill roots already exist under `project`, in preference order.
///
/// Existence is the signal. A repository that already has `.claude/skills` has
/// answered the question of which runtime it is for, and guessing differently
/// would install into a directory nothing reads.
pub fn existing_roots(project: &Path) -> Vec<PathBuf> {
    SKILL_ROOTS
        .iter()
        .map(|root| join_root(project, root))
        .filter(|path| path.is_dir())
        .collect()
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

/// The root an install would use: the first that already exists, else the
/// first known convention.
pub fn target_root(project: &Path) -> PathBuf {
    existing_roots(project)
        .into_iter()
        .next()
        .unwrap_or_else(|| join_root(project, SKILL_ROOTS[0]))
}

/// Every place this skill could already be installed under `project`.
///
/// All roots are checked, not just the preferred one: a skill installed by hand
/// into `.agents/skills` is installed, and reporting it missing would have the
/// agent write a second copy that then drifts from the first.
pub fn installed_paths(project: &Path, name: &str) -> Vec<PathBuf> {
    SKILL_ROOTS
        .iter()
        .map(|root| install_path(&join_root(project, root), name))
        .filter(|path| path.is_file())
        .collect()
}

impl Skill {
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
        let note = self.provenance_note();
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
    fn provenance_note(&self) -> String {
        let r = &self.record;
        if r.origin == Origin::Own {
            return format!(
                "<!-- Authored in {repo}, at {path}.\n     \
                 Part of this devup-mcp build - there is no separate upstream for it to fall \
                 behind.\n     \
                 Current revision: {latest}\n     \
                 Why this matters here: {used} -->",
                repo = r.repo,
                path = r.path,
                latest = r.latest_url,
                used = r.used_for,
            );
        }
        format!(
            "<!-- Vendored from {repo}/{path} at {commit} ({at}).\n     \
             Embedded in this devup-mcp build; no network was used to read it.\n     \
             This revision: {source}\n     \
             Newer revisions, if any: {latest}\n     \
             Why this matters here: {used} -->",
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
    pub fn install_action(&self, project: &Path) -> serde_json::Value {
        let installed = installed_paths(project, &self.record.name);
        if !installed.is_empty() {
            let paths = installed
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            return serde_json::json!({
                "installed": true,
                "paths": paths,
                "action": null,
                "how": "Already installed. Your skill loader picks it up by its own triggers; \
                        nothing to do.",
            });
        }
        match self.record.origin {
            Origin::Embedded | Origin::Own => {
                let roots = existing_roots(project);
                let target = target_root(project);
                let writes = self
                    .record
                    .documents
                    .iter()
                    .map(|document| {
                        document_path(&target, &self.record.name, &document.path)
                            .display()
                            .to_string()
                    })
                    .collect::<Vec<_>>();
                serde_json::json!({
                    "installed": false,
                    "paths": [],
                    "action": "devup_skills",
                    "arguments": {"action": "install", "names": [self.record.name]},
                    "writesTo": writes,
                    "how": "Call devup_skills with action \"install\". The documents are inside this \
                            binary, so it needs no network. Then load it the way your runtime loads \
                            a project skill.",
                    "rootChoice": if roots.is_empty() {
                        format!("No skill root exists yet, so {} is created.", SKILL_ROOTS[0])
                    } else {
                        format!("Using the existing root {}.", target.display())
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
pub fn report(project: &Path) -> serde_json::Value {
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
            let action = skill.install_action(project);
            entry["installed"] = action["installed"].clone();
            entry["installState"] = action;
            entry
        })
        .collect::<Vec<_>>();
    let missing = entries
        .iter()
        .filter(|entry| entry["installed"] == false)
        .count();
    serde_json::json!({
        "workspace": project.display().to_string(),
        "skillRoots": {
            "known": SKILL_ROOTS,
            "existing": existing_roots(project)
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
            "wouldUse": target_root(project).display().to_string(),
        },
        "installedCount": entries.len() - missing,
        "missingCount": missing,
        "skills": entries,
        "how": "These are the conventions for the code devup-mcp emits and reads. Install the \
                missing ones, then let your own skill loader surface them - an installed skill \
                keeps applying for every later session, which is the thing reading a document \
                once does not do.",
        "boundary": "devup-mcp installs only what it carries. It does not download anything and \
                     does not run any install command; for an external skill the command is \
                     yours to run.",
    })
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
pub fn install(
    policy: &super::output::OutputPolicy,
    requested: &[String],
) -> Result<serde_json::Value, devup_mcp_figma::DevupError> {
    use devup_mcp_figma::{DevupError, ErrorCode};

    let project = policy.primary_root().to_path_buf();
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

    let root = target_root(&project);
    let mut transaction = super::output::OutputTransaction::new();
    let mut written = Vec::new();
    let mut already = Vec::new();
    for skill in wanted
        .iter()
        .filter(|skill| skill.record.origin.is_carried())
    {
        let name = &skill.record.name;
        if !installed_paths(&project, name).is_empty() {
            already.push(name.clone());
            continue;
        }
        let documents = skill
            .installable_documents()
            .expect("a carried skill always has documents");
        let mut paths = Vec::with_capacity(documents.len());
        for (relative, contents) in documents {
            let target =
                policy.resolve(&document_path(&root, name, relative).display().to_string())?;
            paths.push(target.display_path().display().to_string());
            transaction.stage(format!("skill:{name}:{relative}"), target, contents.as_bytes())?;
        }
        written.push(serde_json::json!({"name": name, "paths": paths}));
    }
    transaction.commit()?;

    Ok(serde_json::json!({
        "installed": written,
        "alreadyPresent": already,
        "notInstallable": external,
        "root": root.display().to_string(),
        "nextAction": if written.is_empty() && external.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::json!({
                "how": "Load the newly installed skills the way your runtime loads a project \
                        skill, and run any command under notInstallable yourself. Call \
                        devup_skills again to confirm the state changed.",
            })
        },
        "boundary": "Only the documents devup-mcp carries were written. Nothing was downloaded \
                     and no install command was executed.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

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

        let missing = skill.install_action(&project);
        assert_eq!(missing["installed"], false);
        assert_eq!(missing["action"], "devup_skills");

        // Installed by hand into the last root, not the preferred one.
        let root = join_root(&project, SKILL_ROOTS[SKILL_ROOTS.len() - 1]);
        let path = install_path(&root, "devup-ui");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# installed by hand").unwrap();

        let found = skill.install_action(&project);
        assert_eq!(
            found["installed"], true,
            "a hand-installed skill is installed"
        );
        assert_eq!(found["action"], serde_json::Value::Null);
        assert_eq!(found["paths"].as_array().unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&project);
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

        let action = skill.install_action(&project);
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
        let action = skill.install_action(&project);
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
        for skill in all()
            .iter()
            .filter(|skill| skill.entry_text().is_some())
        {
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
            let opener = if skill.record.origin == Origin::Own {
                "Authored in dev-five-git/"
            } else {
                "Vendored from dev-five-git/"
            };
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
}
