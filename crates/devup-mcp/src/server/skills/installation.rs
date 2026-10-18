

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
    ///
    /// `budget` is what is left of the install's time for waiting on
    /// upstream, and only that waiting is taken from it.
    async fn fetch_documents(
        &self,
        upstream: &dyn SkillUpstream,
        budget: &mut std::time::Duration,
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
            if budget.is_zero() {
                return Err((url, timeout_error()));
            }
            let asked = tokio::time::Instant::now();
            let answer = tokio::time::timeout(*budget, upstream.fetch(&url)).await;
            *budget = budget.saturating_sub(asked.elapsed());
            let fetched = answer
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
                include_str!("../text/skills-provenance_note-ca003bce.md"),
                marker = AUTHORED_MARKER,
                repo = r.repo,
                path = r.path,
                latest = r.latest_url,
                used = r.used_for,
            );
        }
        format!(
            include_str!("../text/skills-provenance_note-fa8096ea.md"),
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
                        include_str!("../text/skills-install_action-510f4ba8.md")
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
                    include_str!("../text/skills-install_action-50ca51e1.md")
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
                    "how": include_str!("../text/skills-install_action-e5981815.md"),
                    "runtime": lookup.runtime.as_str(),
                    "rootChoice": if !roots.is_empty() {
                        format!("Using the existing root {}.", targets[0].display())
                    } else if lookup.runtime == Runtime::Unknown {
                        include_str!("../text/skills-install_action-13d23996.md")
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

/// The explicit entry point: `/devup [figma url] [notes]`.
///
/// An MCP server cannot start a skill or a slash command itself, so the user
/// gets one to type. Claude Code turns every project skill into a slash
/// command of the same name, so the `devup` skill installed with the others
/// already is `/devup` there. OpenCode keeps slash commands in their own
/// directory, so it gets a command file as well. Every other runtime is
/// reported unsupported rather than given a path nobody verified.
pub const COMMAND_NAME: &str = "devup";
pub const COMMAND_INVOCATION: &str = "/devup [figma url] [notes]";
/// Marks a command file devup-mcp wrote, so a later install may rewrite it
/// and leaves anyone else's alone.
const COMMAND_MARKER: &str = "<!-- devup-mcp:command devup";
const OPENCODE_COMMAND: &str = include_str!("../commands/opencode-devup.md");
const OPENCODE_COMMAND_ROOT: &str = ".opencode/command";
const OPENCODE_USER_COMMAND_ROOT: &str = ".config/opencode/command";

/// Whether an install writes the OpenCode command: when OpenCode is the
/// client, or when the skills go to its skill root. `roots` are the install's
/// roots as chosen before anything was staged - staging creates directories,
/// and asking again afterwards sees only the first of them.
fn writes_opencode_command(lookup: &Lookup, roots: &[PathBuf]) -> bool {
    lookup.runtime == Runtime::Opencode
        || roots
            .iter()
            .any(|root| root.ends_with(Path::new(".opencode").join("skill")))
}

fn opencode_command_paths(lookup: &Lookup) -> Vec<PathBuf> {
    let file = format!("{COMMAND_NAME}.md");
    let mut paths = vec![join_root(&lookup.project, OPENCODE_COMMAND_ROOT).join(&file)];
    if let Some(home) = &lookup.home {
        paths.push(join_root(home, OPENCODE_USER_COMMAND_ROOT).join(&file));
    }
    paths
}

/// Whether `/devup` is installed for the detected runtime, and where.
pub fn command_state(lookup: &Lookup) -> serde_json::Value {
    let claude = || {
        let paths = Lookup {
            runtime: Runtime::ClaudeCode,
            ..lookup.clone()
        }
        .installed_paths(COMMAND_NAME);
        serde_json::json!({
            "runtime": "claude-code",
            "via": "skill",
            "installed": !paths.is_empty(),
            "paths": paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            "how": "Claude Code offers every project skill as a slash command of the same name; the devup skill is /devup.",
        })
    };
    let opencode = || {
        let found = opencode_command_paths(lookup)
            .into_iter()
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        serde_json::json!({
            "runtime": "opencode",
            "via": "command",
            "installed": !found.is_empty(),
            "paths": found.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            "writesTo": join_root(&lookup.project, OPENCODE_COMMAND_ROOT).join(format!("{COMMAND_NAME}.md")).display().to_string(),
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
        "name": COMMAND_NAME,
        "invocation": COMMAND_INVOCATION,
        "runtime": lookup.runtime.as_str(),
        "supported": supported,
        "installed": installed,
        "perRuntime": per_runtime,
        "how": if !supported {
            format!(
                include_str!("../text/skills-command_state-84cd29d8.md"),
                lookup.runtime.as_str()
            )
        } else if installed {
            "Type /devup with a Figma link and notes to start the whole workflow.".to_owned()
        } else {
            "Call devup_skills with action \"install\"; it writes /devup with the skills.".to_owned()
        },
    })
}

/// Stages the OpenCode command file when this install writes one. A file
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
    let path = join_root(&lookup.project, OPENCODE_COMMAND_ROOT).join(format!("{COMMAND_NAME}.md"));
    let existing = std::fs::read_to_string(&path).ok();
    let state = match existing.as_deref() {
        Some(text) if text == OPENCODE_COMMAND => "alreadyPresent",
        Some(text) if !text.contains(COMMAND_MARKER) => "leftAlone",
        Some(_) => "refreshed",
        None => "installed",
    };
    if matches!(state, "installed" | "refreshed") {
        let target = policy.resolve(&path.display().to_string())?;
        transaction.stage(
            "command:opencode:devup",
            target,
            OPENCODE_COMMAND.as_bytes(),
        )?;
    }
    Ok(vec![serde_json::json!({
        "name": COMMAND_NAME,
        "runtime": "opencode",
        "path": path.display().to_string(),
        "state": state,
        "invocation": COMMAND_INVOCATION,
    })])
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
                    "note": include_str!("../text/skills-report-1a0ab5f8.md"),
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
            "clientName": lookup.client_name,
            "from": "The MCP clientInfo name sent at initialize, echoed here as `clientName`.",
            "why": include_str!("../text/skills-report-071c67f6.md"),
            // A closed-source client's name is matched against what has been
            // observed rather than read out of source, so a wrong guess does
            // not fail - it just never fires, and reads exactly like a client
            // devup-mcp has never heard of. Naming both halves is what lets a
            // reader tell those two apart and say which it is.
            "ifDetectedIsUnknown": if lookup.runtime == Runtime::Unknown {
                serde_json::json!({
                    "meaning": "No runtime matched this client name, so every convention is read \
                                and the three broad ones are written. That is safe, not wrong.",
                    "butAlso": include_str!("../text/skills-report-55e539f1.md"),
                    "reportAt": "https://github.com/dev-five-git/devup-mcp/issues",
                })
            } else {
                serde_json::Value::Null
            },
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
            "userRootsNote": include_str!("../text/skills-report-eb1c9bcc.md"),
            "writeConventions": lookup.runtime.write_roots(),
            "allKnownRoots": EVERY_PROJECT_ROOT,
            "scopeNote": include_str!("../text/skills-report-3e1e10ee.md"),
        },
        "installedCount": entries.len() - missing,
        "missingCount": missing,
        "outdated": stale,
        "outdatedNote": include_str!("../text/skills-report-d3cac7a1.md"),
        "skills": entries,
        "command": command_state(lookup),
        "machineWide": machine_wide(lookup),
        "how": include_str!("../text/skills-report-d4500f82.md"),
        "boundary": include_str!("../text/skills-report-2e1e5387.md"),
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
        "why": include_str!("../text/skills-machine_wide-46b2476c.md"),
        "how": include_str!("../text/skills-machine_wide-85410d98.md"),
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
        "obligation": include_str!("../text/skills-changepacks_obligation-91f6f9c2.md"),
        "createWith": "bunx @changepacks/cli --yes --update-type <major|minor|patch> --message \"<why this change exists>\"",
        "whyNotBare": include_str!("../text/skills-changepacks_obligation-54540b9a.md"),
        "tracks": config.get("ignore").cloned().unwrap_or(serde_json::Value::Null),
        "tracksNote": include_str!("../text/skills-changepacks_obligation-23e2c441.md"),
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
    // One budget for the call, not four seconds multiplied by the registry -
    // and for waiting on upstream only. Writing what was fetched is local
    // work: charged too, a slow disk spent the budget, and the later skills
    // reported it elapsed without ever asking upstream, instead of why they
    // really could not be fetched.
    let mut budget = FETCH_TIMEOUT;
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
                &mut budget,
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
            &mut budget,
            &roots,
            &project,
            &mut warnings,
            &mut written,
        )
        .await?;
    }
    let commands = stage_commands(policy, &mut transaction, lookup, &roots)?;
    transaction.commit()?;

    Ok(serde_json::json!({
        "commands": commands,
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
                "how": include_str!("../text/skills-install_with-94607ffe.md"),
            })
        },
        "boundary": include_str!("../text/skills-install_with-5f02c573.md"),
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
    budget: &mut std::time::Duration,
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
        match skill.fetch_documents(upstream, budget).await {
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
#[path = "fetch_tests.rs"]
mod fetch_tests;