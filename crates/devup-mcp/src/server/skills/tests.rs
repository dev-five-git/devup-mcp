

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn verification_starts_with_lint_fix_then_oxlint() {
        for name in ["devup", "devup-implementation-plan", "devfive-frontend"] {
            let text = EMBEDDED.iter().find(|(key, _)| *key == name).unwrap().1[0].1;
            assert!(text.contains("bun run lint:fix` first, then `bunx oxlint --deny-warnings`"), "{name}");
            assert!(text.contains("Fix only non-fixable leftovers by hand"), "{name}");
        }
    }

    #[test]
    fn user_decisions_override_never_wait_without_prose_questions() {
        let text = include_str!("devup/SKILL.md");
        for required in [
            "This rule takes precedence over",
            "OpenCode `question` or Claude Code `AskUserQuestion`",
            "Do not invent a project-plan `ask` action",
            "Record the chosen answer, not an unaccepted recommendation",
        ] {
            assert!(text.contains(required), "missing decision guidance: {required}");
        }
    }

    /// /devup per runtime: Claude Code gets it as the devup skill, OpenCode as
    /// a command file, and a runtime with no verified command location is
    /// told so instead of given a guessed path.
    #[tokio::test]
    async fn the_devup_command_is_written_where_the_runtime_reads_commands() {
        let project = scratch("devup-command");
        let policy = super::super::output::OutputPolicy::from_roots(vec![project.clone()]).unwrap();
        let opencode = Lookup::project_only(project.clone(), Runtime::Opencode);
        let report = install(&policy, &opencode, &["devup".to_owned()])
            .await
            .unwrap();
        assert_eq!(report["commands"][0]["state"], "installed", "{report}");
        let command = project.join(".opencode").join("command").join("devup.md");
        assert!(command.is_file());
        assert_eq!(command_state(&opencode)["installed"], true);
        let again = install(&policy, &opencode, &["devup".to_owned()])
            .await
            .unwrap();
        assert_eq!(again["commands"][0]["state"], "alreadyPresent");

        let claude = Lookup::project_only(project.clone(), Runtime::ClaudeCode);
        assert_eq!(command_state(&claude)["installed"], false);
        let report = install(&policy, &claude, &["devup".to_owned()])
            .await
            .unwrap();
        assert!(
            report["commands"].as_array().unwrap().is_empty(),
            "{report}"
        );
        let state = command_state(&claude);
        assert_eq!(state["installed"], true, "{state}");
        assert_eq!(state["perRuntime"][0]["via"], "skill");

        let codex = Lookup::project_only(project.clone(), Runtime::Codex);
        let state = command_state(&codex);
        assert_eq!(state["supported"], false);
        assert!(
            state["how"]
                .as_str()
                .unwrap()
                .contains("no command location")
        );
        drop(policy);
        std::fs::remove_dir_all(project).unwrap();
    }

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
            client_name: None,
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

    /// Half the table is matched against names observed in the wild, because
    /// those clients are closed source. A wrong guess does not fail - it just
    /// never fires, and reads exactly like a client devup-mcp has never heard
    /// of. The name has to come back with the verdict or there is no way to
    /// tell those two apart, and no way for the table to get corrected.
    #[test]
    fn the_report_echoes_the_name_beside_the_verdict_it_drew_from_it() {
        let project = scratch("echo");
        let seen = report(&Lookup::new(
            project.to_str(),
            &project,
            Some("some-editor-nobody-added-yet"),
        ));
        assert_eq!(seen["runtime"]["detected"], "unknown");
        assert_eq!(
            seen["runtime"]["clientName"], "some-editor-nobody-added-yet",
            "the name that produced the verdict was dropped"
        );
        assert!(
            !seen["runtime"]["ifDetectedIsUnknown"].is_null(),
            "an unknown client was not told what unknown means"
        );

        // A recognised client needs no such note; saying it anyway is the
        // noise that teaches a reader to skip the field.
        let known = report(&Lookup::new(project.to_str(), &project, Some("codex")));
        assert_eq!(known["runtime"]["detected"], "codex");
        assert_eq!(known["runtime"]["clientName"], "codex");
        assert!(known["runtime"]["ifDetectedIsUnknown"].is_null());

        // Before the handshake there is no name, and claiming one would be a
        // lie about where the verdict came from.
        let silent = report(&Lookup::new(project.to_str(), &project, None));
        assert_eq!(silent["runtime"]["detected"], "unknown");
        assert!(silent["runtime"]["clientName"].is_null());

        let _ = std::fs::remove_dir_all(&project);
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

    /// A project whose package.json has `lint:fix` must hear that formatting
    /// and prop order belong to that script, not to hand edits.
    #[test]
    fn a_lint_fix_script_is_reported_as_an_obligation() {
        let project = scratch("lint-fix");
        std::fs::write(project.join("package.json"), r#"{"scripts":{"lint":"oxlint"}}"#).unwrap();
        assert!(report(&unknown_client(&project))["repoObligations"]["lintFix"].is_null());

        std::fs::write(
            project.join("package.json"),
            r#"{"scripts":{"lint:fix":"oxlint --fix"}}"#,
        )
        .unwrap();
        let found = report(&unknown_client(&project))["repoObligations"]["lintFix"].clone();
        assert_eq!(found["runFirst"], "bun run lint:fix");
        assert_eq!(found["script"], "oxlint --fix");

        let _ = std::fs::remove_dir_all(&project);
    }
}
