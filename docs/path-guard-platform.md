# Output path guard platform investigation

Base: `87ecda8c671a4ce1e012221d2eaaa9775b761038`.
Failed CI: [34612686276](https://github.com/dev-five-git/devup-mcp/actions/runs/34612686276), inspected with `gh run view --log-failed`.

## Verdict: B, with a more precise cause

The reported difference does not demonstrate a traversal bypass. The earlier report's claim that any request containing `..` must fail was incorrect. Both outcomes are safe for this fixture, but the difference is **prefix selection**, not request canonicalization or OS-specific `ParentDir` handling.

`OutputPolicy::from_roots` canonicalizes the configured root with `dunce::canonicalize`, opens a `cap_std::fs::Dir` at that location, and retains both the canonical location and the configured spelling. `resolve` then:

1. Rejects an empty request.
2. For absolute requests, tries component-wise `strip_prefix(display_path)` first, then `strip_prefix(requested_path)` only if the first prefix does not match. Failure to match either rejects the request. Relative requests use the first configured root.
3. Applies `normalize_relative_file` to the remaining suffix. It accepts safe normal components and removes `CurDir`; it rejects `ParentDir`, `RootDir`, and `Prefix`. It does not search the request string for `..`, nor canonicalize the requested file and test its containment.
4. Rejects existing symlink/junction ancestors and builds the reported path from the canonical root and validated suffix.

Staging and commit use that suffix with the preopened `Dir`, recheck symlink ancestors, and reject nonregular existing targets before replacement. The original absolute request is not passed to an ambient file-write API. Accepting a configured alias therefore does not turn its textual prefix into write authority outside the opened root.

### Why the old assertion split

For the old fixture, the configured alias is `<temp-root>/child/..` and the request appends `nested/Component.tsx`.

| Path spelling | Prefix selected | Suffix validated | Result |
| --- | --- | --- | --- |
| Windows fixture with matching canonical temp prefix | canonical `<temp-root>` | `child/../nested/Component.tsx` | rejected: `ParentDir` remains |
| macOS CI `/var/folders/...` resolving under `/private/var/folders/...` | configured `<temp-root>/child/..` | `nested/Component.tsx` | accepted inside opened root |

This is not inherently a Windows-versus-Unix rule: Linux with a direct temp path takes the first branch; a symlinked temp path can take the second. Once the first prefix matches, a suffix validation error does not retry the configured prefix.

In the pinned [dunce 1.0.5 source](https://docs.rs/dunce/1.0.5/src/dunce/lib.rs.html), non-Windows `canonicalize` directly calls `std::fs::canonicalize`. Windows also calls it, then removes the verbatim disk prefix only when safe. Both resolve the existing configured root; neither is called by `resolve` on the output request. Rust's [Path components contract](https://doc.rust-lang.org/std/path/struct.Path.html#method.components) preserves parent components rather than collapsing `a/..`, because symlinks can change their meaning.

## Test correction and boundary coverage

No production behavior, R6–R17 verdict, guidance, or classification changes. No tests are deleted or newly gated by OS.

- `reports_canonical_paths_from_a_noncanonical_root_before_and_after_write` retains its original noncanonical root, absolute/relative canonical output assertions, transaction, and byte checks. Its unrelated platform-dependent rejection assertion is moved into explicit boundary tests.
- `accepts_parent_components_in_a_configured_alias_resolving_inside_the_root` uses `<sandbox>/detour/../allowed`, canonically `<sandbox>/allowed`. The request cannot match the canonical root prefix on any OS, which the fixture asserts. The configured prefix consumes `..`; resolution before and after writing, committed canonical path, and actual bytes must all agree.
- `rejects_parent_components_remaining_after_the_root_prefix` starts from a canonical base on every OS. The overlapping alias `<canonical-root>/child/..` guarantees the first prefix matches and leaves `ParentDir`. Both that absolute request and relative `child/../Component.tsx` must fail, even though their lexical destination is inside the root. This retains the earlier rejection coverage without depending on system temp symlinks.
- `rejects_escapes_from_canonical_and_configured_parent_aliases` checks relative parent escapes, nested parent escapes, canonical absolute escapes, configured-alias escapes, outside absolute paths, and a sibling sharing only a string prefix. It covers overlapping and non-overlapping aliases plus the raw temp spelling, and preserves an outside sentinel.

The existing Unix tests `accepts_a_root_reached_through_a_symlink_in_either_spelling` and `rejects_a_symlink_parent_that_escapes_the_root` remain intact. Both are explicitly `ok` in the supplied macOS run's `tests/output_policy.rs` output. That is evidence from the earlier commit, not a new macOS run. Their configured/canonical prefix checks and symlink-ancestor rejection remain relevant; no new lexical parent collapsing is introduced that could hide a symlink traversal.

These fixtures make prefix selection explicit instead of allowing the host temp spelling to choose an expectation. Their common `ParentDir` validation and root-relative writes support expecting consistent results on Windows, macOS, and Linux. This is code-based reasoning, not a claim of macOS/Linux execution. General permission for arbitrary in-root `a/../file` requests would be a production contract change and is not part of this B fix.

## Verification

Windows, Rust/Cargo 1.98.0; all compiling Cargo commands used `-j 2`.

- Before the unit-test edit, its exact name was run locally: 1 passed, confirming the reported Windows behavior. The supplied macOS CI log provides the failing counterpart; macOS was not reproduced locally.
- `cargo test -p devup-mcp -j 2 --test output_policy -- --nocapture`: exit 0, **8 passed**, including the three new common-platform tests.
- `cargo test --workspace -j 2 --no-fail-fast`: exit 0, **855 passed / 0 failed / 2 ignored**. This is the 852-test baseline plus three added tests.
- `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`: exit 0, **13 passed / 0 failed**.
- `cargo fmt --all -- --check`: exit 0 after formatting the new tests.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: exit 0, **0 warnings / 0 errors**.

MSVC emitted import-library creation messages as `linker_messages` warnings during test linking. They were not suppressed and did not fail the test run.

**자기 target 에서 검증했고 실행된 테스트가 내 것임을 확인했다.** `CARGO_TARGET_DIR` was unset and was not assigned. Cargo metadata reported this worktree's `C:\Users\owjs3\orca\workspaces\devup-mcp\fix-path-guard-platform\target`, verified to be an ordinary directory rather than a link. The full test log identifies compilation from this checkout and execution of `target\debug\deps\devup_mcp-bbcce4741168dc25.exe` and `target\debug\deps\output_policy-e8b9d5ffff431238.exe`. It explicitly records the corrected unit test and all three new integration tests as `ok`. Validation used local Cargo/Node commands, not installed MCP tools.

macOS/Linux post-change results are unverified. The unchanged Unix symlink tests passed in the supplied pre-change macOS log; the platform-independent fixture construction above is the basis for expecting the new tests to pass there, not a substitute for CI execution.
