# UI bundle and structural validation evidence

Scope: the legacy string validation behavior remains unchanged; structural validation is opt-in via `files`.
The bundle is limited to 64 entries and 1,048,576 total UTF-8 bytes including paths, checked before parsing source or themes. All input is supplied in memory, including optional `devup.json`; bundle mode ignores `projectRoot` and never reads the filesystem. Each finding retains the legacy diagnostic fields and adds `path`; `sourceName` remains a caller label.

## RED evidence (recorded before each implementation change)

Initial command: `cargo test -p devup-mcp --test ui_validate_response bundle_ -j 2 -- --nocapture`.
Result: **3 passed, 15 failed**, 19 existing tests filtered out.

Failing tests:
- bundle_accepts_files_without_tsx_and_never_loads_disk_theme
- bundle_ambiguous_query_is_info_and_never_fails_strict
- bundle_app_component
- bundle_barrel
- bundle_default_export
- bundle_inline_style
- bundle_missing_client
- bundle_multiple_components
- bundle_page_name
- bundle_query_missing_local_states_is_error
- bundle_rejects_bounds_before_validation
- bundle_rejects_total_byte_bound
- bundle_unnecessary_client
- bundle_visible_boundaries_downgrade_query_to_info
- bundle_wrapped_prop_handler

Subsequent RED rounds:
- 17 passed / 6 failed: bundle_named_handler_keeps_client_directive, bundle_named_internal_handler_requires_boundary_review, bundle_query_data_not_rendered_abstains, bundle_query_initial_data_downgrades_uncertain_access, bundle_react_namespace_hook_keeps_client_directive, and the still-failing bundle_visible_boundaries_downgrade_query_to_info.
- 25 passed / 2 failed: bundle_nested_component_is_counted, bundle_query_object_passed_to_child_is_info. A Unicode test expectation was corrected from column 25 to 26 before this recorded round; production position calculation was already correct.
- 27 passed / 3 failed: bundle_aliased_suspense_downgrades_query, bundle_class_error_boundary_downgrades_query, bundle_nested_scope_query_does_not_claim_proven_error.
- 0 passed / 1 failed: bundle_internal_inline_handler_with_client_is_valid.

## Added rules and severity policy

| Rule | Policy |
| --- | --- |
| client-boundary | Warning for a visible unnecessary directive or an arrow wrapping a received handler. Missing local directives are info: a parent client boundary may cover the file. No compiler error is claimed. Calls/imports that may require a client boundary make the unnecessary-directive assessment info. Internally defined handlers and aliased/namespace client hooks preserve a needed directive. |
| file-placement | Warning for a JSX component under src/app outside layout.tsx/page.tsx, default exports outside page.tsx, page component names without Page, and re-export-only index.ts/index.tsx barrels. These are convention findings, not claims about compiler validity. |
| one-component-per-file | Warning for additional named components with directly visible JSX returns, including nested definitions. Unproven component factories and arbitrary render helpers are not assumed to be components. |
| react-query-states | Error for directly rendered, nonoptional query-data dereferences without local pending/error guards or visible mitigating evidence. The message explicitly does not assert absence of external boundaries. Info when a supplied boundary, suspense query, child delegation, potentially state-altering options, nested scope ambiguity, optional access, or other control flow prevents proving unsafe rendering. Both visibly handled states produce no finding; data not rendered produces no finding. |
| inline-style | Warning for JSX style object literals, suggesting css classes for statically expressible styles. |

Info never fails strict mode. Errors fail; warnings fail only in strict mode. The existing six `okReason` categories and severity counts are unchanged. Bundle findings never offer an automatic single-string correction using offsets from another file.

## Validation environment

All builds run in this worktree's own `target` with two jobs, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, and `CARGO_INCREMENTAL=0`. `CARGO_TARGET_DIR` is never set. Windows linker informational warnings appear during test linking; workspace clippy is checked separately with `-D warnings`.

Final workspace verification and cleanup results are recorded below when complete.

Additional RED evidence:
- 0 passed / 2 failed in the newly compiled worktree test executable: bundle_conditional_component_is_recognized and bundle_parenthesized_component_is_recognized. Component detection now unwraps parentheses and recognizes visible JSX conditional/logical branches.
- The initial workspace run exposed two existing schema compatibility tests: tools_list_over_raw_stdio_has_no_boolean_schemas_and_object_output_types and exposes_the_seven_read_only_devup_figma_tools. The new files schema used a boolean additionalProperties subschema; it now uses the equivalent object-form `{"not":{}}` so strict input fields remain compatible with those clients.
- Final scope RED round: 33 passed / 2 failed: bundle_handler_parameters_do_not_leak_between_functions and bundle_query_callback_parameter_shadowing_is_info. Handler parameters are now scoped to their function/arrow body, and query access becomes info when callback parameters can shadow its data binding.
- TypeScript syntax RED round: 0 passed / 1 failed: bundle_typescript_files_use_typescript_syntax. Bundle .ts files now use TypeScript parsing; the legacy string entry point delegates to the same implementation with its original TSX parser settings.

## Final verification

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: passed, zero warnings.
- `cargo test --workspace -j 2 --no-fail-fast`: passed, **891 passed / 0 failed / 2 ignored** across 96 reported targets.
- The response integration target has **55 passed / 0 failed**, comprising 19 existing tests and **36 new tests**.
- `git diff --check`: passed.

The string-only route preserves the existing parser settings, serialized result fields, severity behavior, sourceName semantics, and safe correction behavior. The new files field is omitted when absent from serialized retry arguments. No snapshots were modified.
- `cargo insta test --workspace --all-features --check -j 2`: passed; no snapshots to review.

Post-commit cleanup: the completion receipt records the result of `cargo clean` in this worktree. The target was verified as the ordinary local directory `C:\Users\owjs3\orca\workspaces\devup-mcp\W5-ui-structure\target`, with no symlink/shared target or CARGO_TARGET_DIR override.
