/// The slash commands devup-mcp ships, and where each runtime reads them.
///
/// An MCP server cannot start a skill or a slash command itself, so the user
/// gets one to type. Claude Code turns every project skill into a slash
/// command of the same name, so a skill installed with the others already is
/// its command there. OpenCode keeps slash commands in their own directory,
/// so it gets a command file as well. Every other runtime is reported
/// unsupported rather than given a path nobody verified.
///
/// The retrospective command is named here and nowhere else in Rust. Renaming
/// it (to `devup-review-next`, `devup-confirm`, ...) is: change this literal,
/// `git mv skills/devup-retro skills/<new>`, and replace the old name in that
/// skill's frontmatter and its `manifest.json` entry (then update the bytes
/// and sha256). `the_retro_name_is_spelled_in_one_place` fails until all of
/// them agree.
macro_rules! retro_name {
    () => {
        "devup-retro"
    };
}
pub const RETRO_NAME: &str = retro_name!();

pub struct Command {
    pub name: &'static str,
    pub invocation: &'static str,
    /// What typing it does, shown once it is installed.
    ready: &'static str,
    /// Shown to a runtime with no verified command location; `{}` is its name.
    unsupported: &'static str,
    /// A hand-written OpenCode command file, or `None` to build one from the
    /// skill of the same name so the two cannot drift apart.
    opencode: Option<&'static str>,
}

pub const COMMAND_NAME: &str = "devup";
pub const COMMAND_INVOCATION: &str = "/devup [figma url] [notes]";
pub const COMMANDS: &[Command] = &[
    Command {
        name: COMMAND_NAME,
        invocation: COMMAND_INVOCATION,
        ready: "Type /devup with a Figma link and notes to start the whole workflow.",
        unsupported: include_str!("../text/skills-command_state-84cd29d8.md"),
        opencode: Some(include_str!("../commands/opencode-devup.md")),
    },
    Command {
        name: RETRO_NAME,
        invocation: concat!("/", retro_name!(), " [scope or notes]"),
        ready: concat!(
            "Type /",
            retro_name!(),
            " after devup-mcp work is finished: it reviews the session's follow-up prompts, \
             asks you to confirm the list, then prints improvement prompts for devup-mcp."
        ),
        unsupported: concat!(
            "devup-mcp writes no slash command for {}: no command location for it is verified. \
             The ",
            retro_name!(),
            " skill is installed with the others; ask for it by name, or use the MCP prompt of \
             the same name."
        ),
        opencode: None,
    },
];
const OPENCODE_COMMAND_ROOT: &str = ".opencode/command";
const OPENCODE_USER_COMMAND_ROOT: &str = ".config/opencode/command";

impl Command {
    pub fn find(name: &str) -> Option<&'static Command> {
        COMMANDS.iter().find(|command| command.name == name)
    }

    /// Marks a command file devup-mcp wrote, so a later install may rewrite
    /// it and leaves anyone else's alone.
    fn marker(&self) -> String {
        format!("<!-- devup-mcp:command {} ", self.name)
    }

    /// The skill's own title and body under OpenCode's frontmatter.
    fn opencode_text(&self) -> std::borrow::Cow<'static, str> {
        if let Some(text) = self.opencode {
            return text.into();
        }
        let title = find_by_name(self.name).map_or(self.name, |skill| &skill.record.title);
        format!(
            "---\ndescription: {title}\n---\n{}- written by devup_skills install; rewritten by \
             later installs. -->\n\nRun the {} command for: $ARGUMENTS\n\n{}",
            self.marker(),
            self.name,
            skill_body(self.name).trim_start(),
        )
        .into()
    }

    fn opencode_paths(&self, lookup: &Lookup) -> Vec<PathBuf> {
        let file = format!("{}.md", self.name);
        let mut paths = vec![join_root(&lookup.project, OPENCODE_COMMAND_ROOT).join(&file)];
        if let Some(home) = &lookup.home {
            paths.push(join_root(home, OPENCODE_USER_COMMAND_ROOT).join(&file));
        }
        paths
    }

    /// Whether the command is installed for the detected runtime, and where.
    pub fn state(&self, lookup: &Lookup) -> serde_json::Value {
        let name = self.name;
        let claude = || {
            let paths = Lookup {
                runtime: Runtime::ClaudeCode,
                ..lookup.clone()
            }
            .installed_paths(name);
            serde_json::json!({
                "runtime": "claude-code",
                "via": "skill",
                "installed": !paths.is_empty(),
                "paths": paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
                "how": format!("Claude Code offers every project skill as a slash command of the same name; the {name} skill is /{name}."),
            })
        };
        let opencode = || {
            let found = self
                .opencode_paths(lookup)
                .into_iter()
                .filter(|path| path.is_file())
                .collect::<Vec<_>>();
            serde_json::json!({
                "runtime": "opencode",
                "via": "command",
                "installed": !found.is_empty(),
                "paths": found.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
                "writesTo": join_root(&lookup.project, OPENCODE_COMMAND_ROOT).join(format!("{name}.md")).display().to_string(),
                "how": "OpenCode reads slash commands from .opencode/command/*.md.",
            })
        };
        let per_runtime = match lookup.runtime {
            Runtime::ClaudeCode => vec![claude()],
            Runtime::Opencode => vec![opencode()],
            Runtime::Unknown => vec![claude(), opencode()],
            _ => Vec::new(),
        };
        let supported = !per_runtime.is_empty();
        let installed = per_runtime.iter().any(|entry| entry["installed"] == true);
        serde_json::json!({
            "name": name,
            "invocation": self.invocation,
            "runtime": lookup.runtime.as_str(),
            "supported": supported,
            "installed": installed,
            "perRuntime": per_runtime,
            "how": if !supported {
                self.unsupported.replace("{}", lookup.runtime.as_str())
            } else if installed {
                self.ready.to_owned()
            } else {
                format!("Call devup_skills with action \"install\"; it writes /{name} with the skills.")
            },
        })
    }
}

/// A carried skill's entry document without its frontmatter.
pub fn skill_body(name: &str) -> &'static str {
    let text = find_by_name(name)
        .and_then(|skill| skill.texts)
        .and_then(|documents| documents.iter().find(|(path, _)| *path == ENTRY_DOCUMENT))
        .map_or("", |(_, text)| *text);
    &text[frontmatter_end(text).unwrap_or(0)..]
}

/// Whether an install writes the OpenCode commands: when OpenCode is the
/// client, or when the skills go to its skill root. `roots` are the install's
/// roots as chosen before anything was staged - staging creates directories,
/// and asking again afterwards sees only the first of them.
fn writes_opencode_command(lookup: &Lookup, roots: &[PathBuf]) -> bool {
    lookup.runtime == Runtime::Opencode
        || roots
            .iter()
            .any(|root| root.ends_with(Path::new(".opencode").join("skill")))
}

/// Whether `/devup` is installed for the detected runtime, and where.
pub fn command_state(lookup: &Lookup) -> serde_json::Value {
    COMMANDS[0].state(lookup)
}

/// Stages the OpenCode command files when this install writes them. A file
/// devup-mcp did not write is left alone and reported.
fn stage_commands(
    policy: &super::output::OutputPolicy,
    transaction: &mut super::output::OutputTransaction,
    lookup: &Lookup,
    roots: &[PathBuf],
) -> Result<Vec<serde_json::Value>, devup_mcp_figma::DevupError> {
    if !writes_opencode_command(lookup, roots) {
        return Ok(Vec::new());
    }
    let mut reports = Vec::new();
    for command in COMMANDS {
        let path =
            join_root(&lookup.project, OPENCODE_COMMAND_ROOT).join(format!("{}.md", command.name));
        let text = command.opencode_text();
        let existing = std::fs::read_to_string(&path).ok();
        let state = match existing.as_deref() {
            Some(found) if found == text => "alreadyPresent",
            Some(found) if !found.contains(&command.marker()) => "leftAlone",
            Some(_) => "refreshed",
            None => "installed",
        };
        if matches!(state, "installed" | "refreshed") {
            let target = policy.resolve(&path.display().to_string())?;
            transaction.stage(
                format!("command:opencode:{}", command.name),
                target,
                text.as_bytes(),
            )?;
        }
        reports.push(serde_json::json!({
            "name": command.name,
            "runtime": "opencode",
            "path": path.display().to_string(),
            "state": state,
            "invocation": command.invocation,
        }));
    }
    Ok(reports)
}
