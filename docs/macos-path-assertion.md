# macOS CI path assertion correction

Follow-up correction: the claim below that every request containing `..` must
fail was too broad. Run 34612686276 exposed a configured-versus-canonical prefix
selection difference. See [the platform guard investigation](path-guard-platform.md)
for the B verdict, corrected test coverage, and current verification. The results
below describe the earlier commit, not the follow-up.

PR #28, run [34608989645](https://github.com/dev-five-git/devup-mcp/actions/runs/34608989645).
Base: `c77de5d5696b88d2bcb05078bf295f12be52dde4` (`integration/r6`).

## Cause and scope

`gh run view 34608989645 --repo dev-five-git/devup-mcp --log-failed` confirms the failed assertion's actual value starts with `/private/var/folders/` and its expected value with `/var/folders/`; both end in `r13-paths-28672/frame.tsx`.

This is a test expectation bug. `OutputPolicy::from_roots` in `src/server/output.rs` canonicalizes roots with `dunce::canonicalize`. `resolve` accepts either the configured or canonical root spelling, validates the relative suffix, and joins it to the canonical root. `complete_operation` in `src/server/projection.rs` obtains `target.display_path()` and places that exact string in `written_paths`. The implementation deliberately reports canonical locations.

The R13 test now canonicalizes its existing directory before joining the expected filename, following the pre-existing P3 test at original line 4624. Requested paths stay unresolved. The assertion still requires exact string equality, and still checks written content and absence of the unsupported output. No production path behavior, R6–R17 verdicts, guidance, classifications, skips, or ignored tests changed.

`crates/devup-mcp/tests/support/paths.rs` owns the shared `canonical` helper. It is included only in test builds: library unit tests through `#[cfg(test)]`, and output-policy integration tests through a path module. The existing P3 and output-policy comparisons use the same helper. It canonicalizes an existing root, not a possibly unwritten output, and never normalizes the actual returned value (which must already be canonical).

## Exhaustive temp-path audit

Searched all Rust sources under `crates` for `std::env::temp_dir()` and `temp_dir(`, then inspected the enclosing tests and consumers of temporary-directory helpers: **41 actual standard-library calls in 19 files**, excluding three documentation mentions. Locations below refer to the original base.

| File / sites | Finding |
| --- | --- |
| `src/server/projection.rs`: 3308 | Exact returned output path versus unresolved temp path; fixed. |
| Same: 4615 / 4624 | Already canonicalized; shares the helper now. |
| Same: 4734, 4788 | File existence, generated asset URLs and canonical-reference stability; no unresolved absolute output-string comparison. |
| Same: 3048, 5039, 5056, 5122, 5209, 5578 | Output-policy setup only; assertions concern selection, content, resources and guidance, not temporary absolute path spelling. |
| `tests/output_policy.rs` | Returned absolute paths already compare against canonical roots; migrated existing helper. Unix symlink-root and escape tests remain intact. |
| `tests/cli.rs`: 103, 127, 358, 394 | CLI configuration preserves supplied root spelling (`parse_cli_args` pushes the input `PathBuf` unchanged); other sites validate invalid inputs/cache behavior. |
| `tests/ui_validate_response.rs`: 115, 152, 316, 423, 527 | The path equality at 128 verifies preservation of original `projectRoot` in next-action arguments. `validation_guidance::guidance` serializes the original input and changes only TSX. Other sites test theme lookup, source evidence or mode behavior. |
| `src/server/project_root.rs`, `project_context.rs`, `stack_diff.rs` | Scoped-directory helper consumers compare discovered paths that retain the supplied root spelling, relative paths, or domain results. `find_project_root` and scanning join/walk the input without canonicalization. |
| `src/server/validation.rs` | Public URL mapping compares a relative percent-encoded URL, not an absolute filesystem spelling. |
| `src/server/output.rs` | Existing rollback tests check file contents, existence and recovery files. Added explicit normalization coverage below. |
| `src/server/asset_jobs.rs`, `call_cache.rs`; `tests/call_cache_resume.rs` | File fingerprints/states and cache persistence, not canonical-output path equality. |
| `tests/composite_export.rs`, `downstream_integration.rs`, `resource_delivery.rs`, `source_orchestration.rs` | Helper consumers check file bytes, existence, resource publication, rollback, or presence/type of reported paths; no unresolved-versus-canonical absolute equality. |
| `tests/export_defaults.rs`, `ground_truth_tools.rs` | Input/error and project discovery behavior, not absolute output-string equality. |
| `crates/devup-mcp-visual/tests/compare.rs` | Image metrics and diff-file existence, not absolute path equality. |

No additional instance of the reported expectation bug was found. The helper is intentionally for canonical output comparisons; using it on input-preservation assertions would test a different contract.

## Platform evidence and limits

New unit test `reports_canonical_paths_from_a_noncanonical_root_before_and_after_write` creates an existing `child/..` alias whose spelling differs even on Windows. It checks the shared helper collapses the alias, canonical absolute and relative requests resolve to the exact canonical target before that file exists, and transaction commit returns the exact canonical string and writes the expected bytes. A request containing `..` must still fail. The first draft incorrectly expected that request to succeed; direct execution exposed the existing traversal guard, and the test was corrected without changing production code.

This Windows test exercises real filesystem normalization and the production resolve/write pipeline. It does not reproduce the macOS `/var` symlink. The observed macOS failure, the traced `dunce::canonicalize` implementation, matching canonical expected-root construction, and retained Unix symlink-root tests support the expectation that this fixes macOS. macOS CI has not been rerun here; a green macOS run is not claimed.

## Verification

Windows, Rust/Cargo 1.98.0. All compiling Cargo commands used `-j 2`.

- `cargo test --workspace -j 2 --no-fail-fast`: exit 0, **852 passed / 0 failed / 2 ignored** (baseline 851 plus one new unit test).
- `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`: exit 0, **13 passed / 0 failed**.
- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: exit 0, **0 warnings / 0 errors**.

The initial full run used the first draft of the added unit test and returned exit 101 (851 passed, one new test failed, two ignored); the final full run above rebuilt the corrected source. MSVC emitted informational import-library creation messages as `linker_messages` warnings during test linking; these were not test failures and were not suppressed.

**자기 target 에서 검증했고 실행된 테스트가 내 것임을 확인했다.** `cargo metadata` reported `C:\Users\owjs3\orca\workspaces\devup-mcp\fix-macos-path-assertion\target`, with `CARGO_TARGET_DIR` unset. The target directory is not a filesystem link. No installed devup-mcp MCP tool was used. [Binary evidence](macos-path-binary-proof.json) records the workspace, target, absolute executable path, SHA-256, source hash, build timestamp and direct `--exact` executions of both the R13 regression and the new normalization test (one passed each). These direct executions are separate from, and not added to, the workspace count.

Local command logs are `macos-workspace-tests.log` (initial), `macos-workspace-final.log`, `macos-js.log`, `macos-fmt.log` and `macos-clippy.log` in the worktree root. They are ignored by Git. The committed report and binary evidence survive the requested post-commit `cargo clean`.
