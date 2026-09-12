# Route/OpenAPI declaration parsing

## Scope and approach

Replace route attribute quote scanning with Rust AST and attribute argument parsing in stack_diff and stack_diff_parse. Keep the public extraction helper signature for feature_trace; only resolved, unconditional declarations are returned through that helper. Keep legacy text evidence only in a separate low-confidence unresolved finding, never in the definite route count.

The local Vespera 0.3.1 macro source (`vespera_macro/src/args.rs`) accepts a bare HTTP method and named `path = LitStr` in either order, and rejects positional path strings. The parser will follow that contract for comparison keys. Other route options are opaque balanced token values: this tool does not compile or validate every proc-macro option.

## Verification

RED measured before production changes: `cargo test -p devup-mcp --lib route_arguments -j 2` exited 101; **0 passed / 11 failed / 0 ignored**, 297 filtered out.

```text
failures:
    server::stack_diff::tests::route_arguments_cfg_handlers_are_conditional_with_ownership
    server::stack_diff::tests::route_arguments_cfg_module_declaration_gates_child_file
    server::stack_diff::tests::route_arguments_decode_rust_string_literals
    server::stack_diff::tests::route_arguments_do_not_extract_other_namespaces_or_function_bodies
    server::stack_diff::tests::route_arguments_do_not_promote_cfg_modules_or_cfg_attr
    server::stack_diff::tests::route_arguments_ignore_comment_paths
    server::stack_diff::tests::route_arguments_ignore_doc_comment_and_macro_examples
    server::stack_diff::tests::route_arguments_malformed_file_preserves_fallback_evidence
    server::stack_diff::tests::route_arguments_multiline_reversed_and_two_handlers
    server::stack_diff::tests::route_arguments_reject_unsupported_or_ambiguous_paths
    server::stack_diff::tests::route_arguments_unresolved_file_keeps_low_confidence_fallback

test result: FAILED. 0 passed; 11 failed; 0 ignored; 0 measured; 297 filtered out; finished in 0.02s

error: test failed, to rerun pass `-p devup-mcp --lib`

```

Base `c2b4e31944e69035d233f217ce0f1d5fb5e02fb4`, measured in this worktree before implementation: `cargo test --workspace -j 2 --no-fail-fast` completed with **1011 passed / 0 failed / 2 ignored** across 98 reported test suites (including zero-test/doc-test suites).

GREEN after implementation: all 11 original regression tests passed. After the additional malformed UTF-8 regression and fix, `cargo test -p devup-mcp --lib server::stack_diff -j 2` passed **42 / 0 failed / 0 ignored**, including all existing stack-diff normalization and ownership tests. `cargo fmt --all -- --check` passed. `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` passed (exit 0, **0 warnings**). `cargo test --workspace -j 2 --no-fail-fast` passed (exit 0): **1023 passed / 0 failed / 2 ignored**, across 98 suites, a net addition of 12 tests over the measured base. `cargo insta test --workspace --all-features --check` passed (exit 0): **1023 passed / 0 failed / 2 ignored**, with **no snapshots to review**. Its build concurrency was bounded with `CARGO_BUILD_JOBS=2`.

All builds use this worktree's own `target`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`, and two build jobs. `CARGO_TARGET_DIR` was never set. Windows test linking emits existing `linker_messages` warnings containing the linker's localized import-library creation message, also observed on the unmodified base; these are separate from Clippy's required zero-warning result.

## Confidence and ownership

No finding will be raised above medium. Parsing does not prove completeness across macro expansion, route merges, re-exports, aliases or build configuration. Existing route-missing-from-openapi stays medium and openapi-path-not-found-in-scanned-routes stays low; conditional and unresolved evidence is low. Source ownership remains attached by the existing common layer finalizer: review authored handlers/configuration, rebuild to regenerate openapi.json, then regenerate the downstream client.

## Additional RED evidence

Malformed-input review identified a potential byte-boundary panic in the retained legacy fallback. Before fixing it, `cargo test -p devup-mcp --lib route_arguments_unfinished_unicode -j 2` exited 101 with **0 passed / 1 failed / 0 ignored**, test `server::stack_diff::tests::route_arguments_unfinished_unicode_attribute_does_not_panic`: start byte index 9 is inside the UTF-8 character 경.

## Accepted and rejected evidence

- Accepts `#[vespera::route(...)]`, `#[route(...)]`, whitespace around namespace punctuation, multiline arguments and attributes, multiple handler attributes, multiple public async handlers, reversed method/path order, and method-only module-root routes.
- Uses `syn::LitStr::value()` for ordinary/raw strings, escaped quotes, Unicode/hex escapes and string continuations. Comment/doc-comment text, nested block comments, unrelated strings/options/macros and function bodies cannot supply a declaration or path.
- Rejects positional path strings (not supported by inspected Vespera), non-string values, byte strings, computed paths such as constants and `concat!`, duplicate paths/methods, other macro namespaces, and handlers that are not public async functions. Unresolved route arguments or whole-file syntax errors produce the separate fallback finding.
- Direct cfg/cfg_attr, file inner cfg, inline module cfg, and ordinary out-of-line cfg module declarations within scanned route trees are conservative conditional evidence. Conditions are not evaluated; even a condition that happens to be true in one build is not promoted to unconditional evidence.
- `conditionalRouteCount` is separate from `codeRouteCount`; conditional routes do not count toward spelling-normalized matches or missing-from-spec findings. Their known spec keys also avoid a contradictory not-found-in-scanned-routes finding.
- `route_url_prefix`, `join_route_url`, route comparison keys, client camelCase/kebab-case folding and all existing normalization tests are unchanged, preserving the existing module-root/trailing-slash behavior and `spellingNormalizedMatches` contract.

## Limits and integration notes

This remains a bounded source scan. It does not expand macros, resolve re-exports/type aliases, prove handler registration, discover merged sub-app routes, evaluate cfg predicates, or validate every unrelated proc-macro option. Module gates follow conventional paths within scanned route trees; arbitrary `#[path]` redirections or gates outside those trees are not resolved. Other route option values are skipped as balanced Rust token trees, so their quoted strings cannot become paths.

The only dependency change is adding already-locked `syn 2.0.119` with `full` parsing support to devup-mcp and its corresponding Cargo.lock dependency edge. No forbidden source files were edited. Suggested server/mod.rs description update for the coordinator: describe route/OpenAPI evidence as parsed Rust method/path declarations with separate conditional and unresolved low-confidence evidence, while retaining the medium confidence cap and rebuild-to-regenerate repair direction.

## Finding confidence decisions

| Finding | Confidence after change | Decision |
| --- | --- | --- |
| route-missing-from-openapi | medium | Unchanged: a local parsed key does not prove complete registration/scan coverage. |
| openapi-path-not-found-in-scanned-routes | low | Unchanged: generated paths may originate in merged sub-apps or unscanned expansion. |
| mirrored-drift self-check | low | Unchanged: spelling similarity is an unverified check. |
| conditional-route-openapi | low | New: cfg/cfg_attr may compile declarations out. |
| route-openapi-unresolved-fallback | low | New: retained text candidates are explicitly unverified and excluded from definite counts. |

**Findings raised above medium: none.** No finding establishes both scan completeness and two direct local declarations. All other layer finding kinds and confidence values are unchanged.

## Delivery

The implementation and this report are committed together on the dispatched worktree branch; no push is performed. The terminal completion report supplies the exact commit hash and the result of `cargo clean`, which runs after committing to release this worktree's build artifacts.
