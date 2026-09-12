# UI reuse context implementation report

Adds opt-in `scope: "ui"` to `devup_project_context`, without adding it to `all`.
The only router change is its tool description. The scope field advertises the
new enum value and filter semantics. All scanning and parsing stay in sibling
modules. The coordinator approved the additive shared-scanner entry point and
four existing workspace OXC dependencies; Cargo.lock adds only those four edges.

## RED evidence

All commands used this worktree's own `target`, with `CARGO_TARGET_DIR` unset,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`,
and `-j 2`. Full local RED logs are `ui-red.log`, `ui-edge-red.log`,
`ui-final-red.log`, `ui-usage-red.log`, and `ui-relative-red.log` (ignored build
artifacts); the durable failure record follows.

Initial command: `cargo test -p devup-mcp ui_tests -j 2 --no-fail-fast`.
Result before implementation: **0 passed / 14 failed / 0 ignored**. Thirteen
failures rejected the unsupported `ui` scope; the schema test failed because
`ui` was absent from the enum. Failing names, all under
`server::project_context::ui_tests::`:

- `ui_client_component_records_exported_names_and_declared_props`
- `ui_server_component_ignores_use_client_in_comments_and_strings`
- `ui_import_sites_resolve_aliases_barrels_and_transitive_routes`
- `ui_route_tree_preserves_route_groups_and_page_components`
- `ui_all_remains_opt_in_and_scope_schema_explains_it`
- `ui_component_cap_is_explicit`
- `ui_total_byte_cap_is_explicit_and_bounds_serialized_response`
- `ui_excludes_all_checkout_kinds_and_reports_exact_reasons`
- `ui_filter_is_literal_case_sensitive_substring`
- `ui_devup_reexports_are_distinguished_from_project_components`
- `ui_multi_app_authority_is_package_local`
- `ui_unresolved_props_and_parse_errors_are_explicit`
- `ui_wrapped_components_and_local_intersection_props_are_evidence`
- `ui_barrel_imports_do_not_invent_usage_of_other_exports`

Additional RED command: `cargo test -p devup-mcp --lib ui_tests -j 2`.
Result before fixes: **14 passed / 4 failed / 0 ignored**:

- `ui_default_class_component_props_are_not_silently_omitted`: default class absent.
- `ui_type_only_imports_do_not_claim_component_usage`: type-only import falsely reported usage.
- `ui_jsonc_aliases_resolve_without_reading_excluded_sources`: commented/trailing-comma config failed alias resolution.
- `ui_export_star_does_not_forward_default_exports`: export-star incorrectly forwarded default.

Final RED command: `cargo test -p devup-mcp --lib ui_tests -j 2`.
Result before fixes: **18 passed / 3 failed / 0 ignored**:

- `ui_unterminated_jsonc_is_reported_without_panicking`: `/*` config caused an index panic.
- `ui_generic_props_are_not_claimed_fully_resolved`: generic alias incorrectly marked resolved.
- `ui_alias_exported_component_keeps_local_props`: lowercase local exported as `Button` was omitted.

Usage-edge RED command: `cargo test -p devup-mcp --lib ui_tests -j 2`.
Result before fixes: **21 passed / 2 failed / 0 ignored**, recorded in
`ui-usage-red.log`:

- `ui_importing_a_value_helper_does_not_claim_component_usage`: importing a non-component helper incorrectly counted as importing the component from that file.
- `ui_layouts_outside_app_do_not_acquire_route_membership`: a root-level layout outside App Router incorrectly inherited the app page's route.

The JSONC test also verifies that a path alias aimed at a malformed nested
checkout source is reported as unresolved/excluded, with zero unparsed files:
the stale source was never read.

Relative-root RED command:
`cargo test -p devup-mcp --lib ui_relative_project_root_resolves_the_same_import_evidence -j 2`.
Result before the fix: **0 passed / 1 failed / 0 ignored**, recorded in
`ui-relative-red.log`. The named test observed an empty import-site array with
a `./` project root versus the correct `/` route using its absolute equivalent.
Normalizing scanned lookup keys the same way as resolved import paths fixes it.

## Published response shape

This is the scope payload. The existing MCP delivery helper additionally adds
`server` identity and the normal MCP transport envelope. Optional properties
are marked in the prose below, not silently absent due to filtering.

```typescript
{
  found: boolean,
  scope: "ui",
  projectRoot: string,
  components: Array<{
    path: string,
    exports: string[],
    localNames: string[],
    client: boolean,
    kind: "project-component" | "devup-ui-reexport" | "project-reexport" | "mixed",
    props: Array<{component: string, name: string, optional: boolean, type: string | null}>,
    propsStatus: "resolved" | "unresolved" | "not-declared-here",
    propsDeclarations: Array<{component: string, type: string | null, resolved: boolean}>,
    reexports: Array<{exported: string, imported: string, source: string}>,
    authority: "project-root" | "package-local",
    appliesTo: string,
    importedBy: Array<{
      path: string,
      routes: string[],
      routePages: string[],
      authority: "project-root" | "package-local",
      appliesTo: string
    }>
  }>,
  routes: Array<{
    route: string,
    page: string,
    routeGroups: string[],
    parallelSlots: string[],
    components: string[],
    authority: "project-root" | "package-local",
    appliesTo: string
  }>,
  excludedPaths: Array<{path: string, reason: "nested-checkout-directory" | "nested-git-checkout"}>,
  diagnostics: Array<
    {path: string, kind: "parse-error", state: "unparsed", reason: string, authority: string, appliesTo: string} |
    {path: string, kind: "config-unresolved", reason: string} |
    {path: string, kind: "import-unresolved", source: string, reason: string}
  >,
  scannedFiles: number,
  unparsedFiles: number,
  componentCountExact: boolean,
  totalComponents: number,
  matchedComponents: number,
  totalRoutes: number,
  matchedRoutes: number,
  totalExcludedPaths: number,
  totalDiagnostics: number,
  limits: {
    maxComponents: 200,
    maxBytes: 131072,
    maxSourceBytes: 2097152,
    byteEncoding: string,
    componentCountUnit: string
  },
  truncation: {
    truncated: boolean,
    capsHit: Array<"maxComponents" | "maxBytes">,
    omitted: {components: number, routes: number, diagnostics: number, excludedPaths: number}
  },
  evidenceNotes: string[],
  authorityNote?: string,
  guardrail?: {action: "stop-and-report", message: string}
}
```

If project-root discovery itself fails, the existing shared not-found guardrail
envelope is returned unchanged.

`totalComponents` counts parsed file entries with component exports before
filtering; `matchedComponents` is after filtering and before output caps.
Likewise for routes. `client` records the file's actual directive prologue,
not transitive membership in a client bundle. Props types are exact source
spans (local interface/type-literal/intersection declarations can be expanded).
Imported and generic types remain explicitly unresolved, preserving the written
annotation and any directly declared fields. A named re-export identifies both
the original imported name and its source package. Wildcard reexports retain
`*` as wildcard evidence rather than inventing names from an external package.

Import sites follow static relative imports, tsconfig/jsconfig baseUrl and
paths, .js-to-TypeScript extension resolution, named/default imports, barrel
re-exports and namespace imports. JSONC comments/trailing commas are supported.
`routes` on an import site indicates static module reachability, including
layouts/templates, not proof a component renders at runtime. `routePages`
disambiguates identical URL paths in multiple applications.

The route tree is a flat list of App Router page leaves: each carries its full
route path, page source path, route groups, parallel slots, and candidate files
under the corresponding `components/pages/<route>/` subtree (including `src/`).

Filtering is literal, case-sensitive substring matching on component paths,
exported/local names, route paths and page paths. Diagnostics and exclusions
remain visible even when the filter matches nothing. `all` directly retains
only theme/API/DB; `ui_all_remains_opt_in_and_scope_schema_explains_it` asserts
that neither `ui` nor `components` is present in its actual response.

## Bounds, exclusions, and limitations

The component-entry cap is 200. The total compact UTF-8 JSON scope payload cap
is 131,072 bytes, excluding the existing server-identity/transport envelope.
`truncation.capsHit` explicitly names `maxComponents` and/or `maxBytes`.
The response keeps exact omitted counts for each array; exclusions and
diagnostics cannot disappear silently if they themselves exhaust the byte
budget. Each input file is limited to 2 MiB; larger/unreadable/malformed files
produce an `unparsed` diagnostic with path and reason. `unparsedFiles` and
`componentCountExact: false` warn that additional components may exist there.

The predicate entry point delegates to the existing scanner implementation.
The exclusion lists remain single-sourced, with unchanged traversal, sorting,
depth behavior for existing named scans, and symlink handling. Dependency/build
directories are pruned exactly as before: node_modules, target, dist, build,
out, .next, .turbo, .nuxt, .venv, venv, __pycache__, .cache, coverage, .git.
Nested .worktrees/.worktree/.git-worktrees candidates are reported with
`nested-checkout-directory`; nested .git checkouts use `nested-git-checkout`.
Excluded source contents are never read. Aliases resolve only against the
authoritative scanned file set, so they cannot pull stale worktree code back in.

This is syntactic reuse evidence, not a complete TypeScript type checker or a
runtime render analysis. Dynamic imports, package export maps, and inherited
tsconfig paths are not evaluated; inherited configuration and unresolved local
imports produce diagnostics. Unknown prop types are never declared resolved.

## Workspace verification

All final commands completed successfully in this worktree:

| Command | Observed result |
| --- | --- |
| `cargo fmt --all -- --check` | Exit 0 |
| `cargo clippy --workspace --all-targets -j 2 -- -D warnings` | Exit 0; 0 warnings |
| `cargo test -p devup-mcp --lib ui_tests -j 2` | 24 passed, 0 failed |
| `cargo test --workspace -j 2 --no-fail-fast` | 910 passed, 0 failed, 2 ignored |
| `cargo test -p devup-mcp --test stdio_smoke -j 2` | 2 passed, 0 failed |
| `git diff --check` | Exit 0 |

The 910 workspace passes include the 24 new UI regressions. The coordinator
corrected the task's originally supplied 890 baseline to **886** for base commit
`c662c4f4acb2fc579e0121cd727b17478c501e07`; the observed total is exactly
886 + 24. No pre-existing tests were removed or ignored. The standalone smoke
passes are reported separately and are not added a second time to the total.
Workspace module-boundary tests pass; scanning did not migrate into the router.

The final command logs are `ui-green.log`, `ui-clippy.log`,
`ui-workspace-tests.log`, and `ui-stdio-smoke.log`. Windows test linking emits
informational import-library linker notices; the required Clippy run has zero
warnings. Cargo metadata confirmed the target directory is exactly
`C:\Users\owjs3\orca\workspaces\devup-mcp\W3-reuse-context\target`.

The coordinator independently reviewed authority, exclusions, bounds and
unknown-evidence reporting, and approved the additive scanner/dependency scope
extensions. The final orchestration completion receipt records the local commit
and the subsequent worktree-local `cargo clean`; nothing is pushed.
