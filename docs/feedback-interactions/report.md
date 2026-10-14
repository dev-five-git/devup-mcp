# Feedback 002/005 interaction implementation

Implementation commit: `17689e9a1401c932e2d25855672e962d3474968e`.
Branch: `devfive-relay-c70423/feedback-interactions`.
Date: 2026-10-14. Task: `task_db92b89020b4`, dispatch: `ctx_a0f28461e0fc`.
This report is committed separately after the tested implementation so its code commit SHA is exact.

The owned tool changes are implemented and locally verified. Exports now surface interaction work that static markup leaves unresolved, expose existing application context without environment values, and require accessible wiring and verification before functional completion. No production destination, record ID, provider credential or application behavior is invented. Prime's actual candidate count, rendered page, clicks, widgets and visual fidelity have **not** been verified by this worker.

## Source and scope

Read the original feedback files as UTF-8 from `C:/Users/devfivedevleader/orca/workspaces/devfive-center/margate/devup-mcp-feedback`:

| File | SHA-256 |
| --- | --- |
| `002-missing-links-interactions.txt` | `1994A12D7E679258A60952E7F86A1FAAE162CDBEABFCC27C9D7F56DD127B3470` |
| `005-one-shot-improvements.txt` | `75E51337E852784358ED395899A2616CB2113A0A8439D9CB995941D9B3799933` |

Repository README, Cargo/workspace configuration, CI/changepack policy, existing projection/review/validation/trace/context facilities and skill installation/provenance code were reviewed. No repository `AGENTS.md` was present. Feedback 002 additions 8–9 belong to the theme worker; feedback 005 item 3 and asset/plugin export behavior belong to the asset worker. This implementation does not change theme projection, responsive grouping, assets/plugin exports or Figma URL parsing.

## Shipped contract and evidence boundaries

- `devup_figma_export` adds a typed, optional `interactiveCandidates` output schema with `nodeId`, `name`, `text`, role (`link`, `button`, `tab`, `nav`, `checkbox`, `radio`), reasons, prototype actions, `hasPrototypeTarget`, `targetResolved` and unresolved reasons. Modern `reaction.actions` and legacy `reaction.action` are preserved. Timer animation reactions are excluded from user-action candidates.
- Prototype destinations, overlays, URLs and active/disabled/selected variant evidence appear in structured metadata and trailing `// @devup-interaction {...}` comments in the actual TSX. Comments preserve existing generated element/source-map positions and do not add handlers, native semantics or React state. A Figma destination node ID alone never becomes a guessed route. Unverified detail-card IDs remain explicit unresolved data.
- `DEVUP_CODEGEN_INTERACTION_UNMAPPED` is grouped by screen with candidate node IDs and produces a **required** checklist item. Failed reads and truncated evidence have separate required issues. Such issues set `deliverable.isFinal=false` while preserving existing visual projection/status semantics. Existing visual quality does not certify functionality.
- Shared component groups use explicit `mainComponentId`/`mainComponentSetId` (and compatible older identity fields), with instance labels/counts, state variants and differences. Identical names alone are insufficient evidence. `mainComponentName` contributes semantic role hints. The companion asset worker supplies these new acquisition fields; this commit consumes them without changing acquisition scripts.
- Widget candidates include name hints for CAPTCHA/maps/payments and conservative CAPTCHA/map/payment composition hints. All are unverified candidates. State/icon details retain captured border, radius, padding, stroke/fill and variants; filled/outline names remain hints, not verified vector classification. Missing fields are not fabricated.
- With explicit `projectRoot`, export merges the existing UI/component/import/route inventory and bounded environment **key names only**, plus suggestions drawn from real inventoried routes. Header/Footer/widget reuse remains a compatibility decision. Suggestions never set `targetResolved=true` or supply an absent record ID.
- `devup_project_context(scope="routes")` provides the existing Next App Router page inventory and its authority, groups, slots, parse/cap/exclusion evidence. It remains opt-in and excluded from `all`.
- `devup_ui_validate` adds action/accessibility warnings, missing static internal App Router destination errors, and literal active-state info. It supports imported Next Link aliases, anchor/native-control wrappers, concrete dynamic/catch-all route matches and query/hash removal. Missing-route errors are skipped for incomplete/ambiguous inventories, bundles that cannot prove absent files, detected custom routing, or missing app evidence. Known route handlers/public files are considered. Relative/external/hash/mailto links and unknown href expressions are not falsely declared missing.
- `navigation.coverage` separates candidate count, semantic controls, wiring evidenced in source, native checkbox/radio toggling and dynamic destinations. Bare native buttons stay behavior-unresolved. `renderedHtml` accepts caller-captured HTML and reports anchor counts separately, excluding script/style/comment templates. These counts do not establish successful clicks or a complete census of rendered controls.
- `devup_feature_trace.navigationGraph` reports code href edges for the selected source slice, with page-route origins when supplied, and compares cached Figma actions or parsed caller-declared export comments. `navigationTargets` explicitly maps Figma destination IDs to code paths. Rows distinguish static destination presence, missing literal href, an explicit node binding mismatch, and unverified node targets. URL equality does not prove control-level binding or runtime navigation.
- Grouped `ambiguityQuestions` cover unknown destinations/actions/data, shared menu versions, differing logo names and providers. Explicit design URLs do not create destination-ambiguity questions merely because source wiring is unverified. Completion evidence calls for candidate/wired/tested counts, unresolved reasons, implemented/remaining widths and actual screenshot comparison.

Interaction metadata is capped at 128 candidates, 64 detail/group entries and 65,536 UTF-8 bytes with named omission counts and required review. Component state fields and repeated instances have declared caps. Per-frame outputs reference the shared candidate inventory rather than duplicating it. Merged project context has a separate size/remaining-wire budget, reports omissions, and explicitly defers to the context tool when necessary. Existing inline/resource limits are unchanged.

## Regression references

Unless otherwise stated these are in `crates/devup-mcp/src/server/feedback_interactions_tests.rs`. All listed tests passed.

| Ref | Test |
| --- | --- |
| T1 | `feedback_interactions_preserve_targets_roles_states_and_unresolved_data` — typed output schema, roles, state, NAVIGATE/OVERLAY/URL/legacy action, record-ID uncertainty and timer exclusion |
| T2 | `feedback_shared_widgets_details_and_grouped_questions_require_review` — master/state differences, widget/detail hints, grouped questions, required review, static completion and missing identity |
| T3 | `feedback_context_routes_reuse_env_names_and_no_secret_values` — route groups/dynamic paths, Header reuse, key names without values, nested checkout exclusion and noninvented suggestions |
| T4 | `feedback_validation_catches_missing_routes_unmapped_actions_and_active_state` — `/inquiry` error, unmapped/accessible controls, literal active info, distinct source/rendered counts |
| T5 | `feedback_validation_limits_do_not_claim_absence_or_success` — incomplete/ambiguous/custom inventories, dynamic hrefs, external/hash links, parse failure and catch-all rules |
| T6 | `feedback_validation_merged_verdict_and_bundle_disk_independence` — errors/strict warnings affect `ok` and bundle mode does not read the supplied disk root |
| T7 | `feedback_navigation_graph_compares_explicit_targets_without_inference` — matched, missing, explicit mismatch and unverified prototype destinations |
| T8 | `feedback_widget_compositions_and_large_metadata_are_bounded` — unnamed CAPTCHA composition, map/payment name candidates, byte/count caps and required truncation review |
| T9 | `feedback_native_semantics_hrefs_and_html_do_not_overstate_wiring` — bare buttons, native toggle versus handler/form wiring, dynamic targets, mailto/query/hash, descriptive prose and HTML script/comment exclusion |
| T10 | `feedback_route_handlers_public_files_and_custom_routing_avoid_false_errors` — valid route handlers/extensionless public files and custom routing skip |
| T11 | `feedback_feature_trace_reads_real_export_hints_and_source_slice` — actual trace entry point, parsed export comments, explicit mapping and page-origin edges |
| T12 | `feedback_boolean_state_and_form_context_keep_bare_buttons_unresolved` — boolean active/disabled state, hidden inputs and submit versus local buttons |
| T13 | `feedback_context_budget_caps_or_defers_explicitly` — bounded or deferred context within the remaining serialized MCP budget |
| T14 | `feedback_read_failures_and_direct_selection_preserve_review_and_source_maps` — direct-root scope, read errors, intact source maps and required checklist survival under caps |
| R | `projection::w1_regressions::feedback_interaction_hints_match_saved_files_and_resource_hashes` — single/Section output files equal resource chunks, byte length/SHA-256 agree, comments retain state/URL hints, mapping verdict unchanged |
| S | `skills::fetch_tests::feedback_frontend_rules_survive_available_and_failed_upstreams_without_duplicates` — installed link/Link-wrap/usePathname/Windows/visual guidance, honest own provenance and idempotence despite available or failed upstreams |
| M | `tests/ground_truth_tools.rs::feedback_navigation_rules_and_routes_scope_reach_mcp_clients` — real MCP calls accept routes scope/renderedHtml and return missing-route errors, candidate warnings, counts and failed validation verdict |

`devfive-frontend` was already `origin: own` in the manifest. `skills.rs` fetches only `Origin::Embedded`; own documents always come from the binary. `scripts/refresh-skills.mjs` reseals own-document digests without replacing their content. The supported architecture was preserved and the manifest digest updated. S additionally proves an obsolete successful upstream response cannot replace these rules, the failing upstream path behaves identically, and repeated installation does not duplicate the checklist. No upstream provenance was invented.

## Feedback coverage matrix

| Owned proposal | Implementation / evidence | Live-only or precise remaining boundary |
| --- | --- | --- |
| 002 A1 prototype hints | Structured actions and actual TSX comments; T1, R | Hints require application wiring; no automatic URL-to-route inference |
| 002 A2 interaction loss/checklist | Screen-grouped unmapped issues, required review; T1, T2, T8, T14 | Heuristics are review candidates, not a complete live-control detector |
| 002 A3 typed candidates | Export output schema and typed roles/target reasons; T1, T2 | Native accessible controls are implemented by the consuming application |
| 002 A4 repeated/shared components | Explicit master/set groups and differences; T2, T3 | Fresh companion-plugin acquisition is needed for previously absent master identity |
| 002 A5 inventory/suggestions | Routes scope, authority and actual-route suggestions; T3, T10, M | Suggestions require confirmation, especially record IDs and package authority |
| 002 A6 validation rules | Source AST and route checks, active info; T4–T6, T9, T10, T12, M | A plain fixed 3px underline without semantic state is explicitly not reliably classified; route/custom-wrapper/runtime limits are disclosed |
| 002 A7 page navigation graph | Explicit target mapping and graph comparison; T7, T11 | Dynamic/helper/wrapper links and unmapped Figma IDs remain unverified |
| 002 B1 frontend link checklist | Own frontend skill including native roles, Link wrapping, path-based active state, absent-route handling; S | No downstream Next application build/typecheck performed by this worker |
| 002 B2 MCP instructions | Export is static; resolve candidates and validate before completion; existing guide instruction tests plus S | Server instructions guide consumers; they cannot prove a consumer executed browser checks |
| 002 B3 completion report rules | Candidate/wired/native/dynamic counts and visual/runtime requirements; T2, T9, R, S | All exported/runtime flags remain unverified until actual evidence exists |
| 002 C1–C5 recommended agent workflow | Enumeration, route table, confirmed destinations, path state and validation are in the installed checklist; T1, T3, T4, S, M | Actual Prime implementation/review is outside this MCP repository change |
| 002 additional 1 repeated `/inquiry` | Missing static path is an error through MCP; T4, T6, M | The five production occurrences were not edited or live-tested here |
| 002 additional 2 missing card IDs | `targetResolved=false`, explicit data uncertainty and dynamic-route suggestions; T1, T3 | Production card ID/data contracts must be supplied |
| 002 additional 3 `as={Link}` TS2322 | Installed guidance uses Link around Box/Flex, with flex style on Link; S | Not a claim that every downstream library/version has been typechecked |
| 002 additional 4 Windows/tooling | UTF-8 script files, LiteralPath for `[id]`, portable search/head alternatives; S and M's `[id]` fixture | Human/agent shell guidance; no claim that unavailable binaries were installed |
| 002 additional 5 unknown 소개 destination | Empty actual-route suggestion, unresolved target and grouped confirmation; T3 | No `/` fallback or new route is synthesized |
| 002 additional 6 fixed active GNB | Literal active/boolean attributes produce info; T4, T12, S | Visual-only 3px active underline remains explicitly unverified without semantic markers/browser evidence |
| 002 additional 7 rendered href census | Separate source and supplied rendered HTML counts; T4, T9, M | Prime's actual rendered census and click success are not measured |
| 002 additional 8 responsive groups | Theme worker ownership | Excluded from this commit; report implemented/remaining widths in completion evidence |
| 002 additional 9 token/fill/hug projection | Theme worker ownership | Excluded from this commit |
| 002 additional 10 screenshot comparison | Required visual evidence and screenshot/environment workflow; T2, S | Actual Prime screenshot/reference comparison not run |
| 005 item 1 shared GNB/version differences | Explicit master/set instance comparison, labels/counts, required shared review; T2, T3 | New plugin reload/acquisition needed; site-wide version remains a confirmation decision |
| 005 item 2 controls/prototype/state | Checkbox/radio/tab/nav roles, actions and active/disabled/selected comments; T1, R, S | Static hint reflection does not claim stateful React/form behavior |
| 005 item 3 asset export failures | Asset worker ownership | Excluded from this commit |
| 005 item 4 external widgets | Name and composition candidates, required integration review/provider questions; T2, T8 | Actual CAPTCHA/maps/payment setup, credential values and behavior are not supplied or tested |
| 005 item 5 states/icons/early compare | Captured chip/check/icon details and variants plus compare/completion guidance; T1, T2, R, S | Pixel shape/icon fidelity needs actual screenshots; unknown styles remain unresolved |
| 005 item 6 project context | Reused UI inventory, existing components/routes and env names merged into export; T3, T10, T13 | Context caps/defer state are explicit; env key presence does not establish valid provider configuration |
| 005 item 7 grouped ambiguities | Destination/data, shared variants, differing logo names and widget topics returned together; T2 | Consumers must answer the unresolved decisions; no guessed answer is treated as approved |

## Validation result

The implementation passed **423 library tests plus 22 integration tests** (445 total), workspace clippy with all targets/features and `-D warnings`, rustfmt check, and staged whitespace validation. Seventeen new regression tests cover this work. Existing production projection/resource/verdict-scope fixtures also pass. See [checks.json](checks.json), [library-tests.txt](library-tests.txt), [integration-tests.txt](integration-tests.txt), [clippy.txt](clippy.txt) and the empty successful [fmt-check.txt](fmt-check.txt).

Exact commands:

```text
cargo test --locked -p devup-mcp --lib -- --skip symlink_escape_refused_before_read
cargo test --locked -p devup-mcp --test ground_truth_tools --test module_boundaries --test export_project_theme --test stdio_smoke
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --cached --check
```

The initial unfiltered library run encountered the existing `visual_compare::tests::symlink_escape_refused_before_read` Windows error **1314** at symlink creation (“A required privilege is not held by the client”). The final library run excludes exactly that unrelated environment-dependent test. No unfiltered full-workspace test pass, live Figma acceptance, production browser interaction, real widget integration or Prime visual comparison is claimed. The rendered HTML parser reports an explicit XML-compatible HTML boundary; malformed HTML can be incomplete rather than a passing browser result.

## Files and integration

Implementation commit modifies/adds these files:

```text
.changepacks/changepack_log_feedback_interactions.json
Cargo.lock
crates/devup-mcp/Cargo.toml
crates/devup-mcp/src/server/feature_trace.rs
crates/devup-mcp/src/server/feedback_interactions_tests.rs
crates/devup-mcp/src/server/guide.rs
crates/devup-mcp/src/server/interactions.rs
crates/devup-mcp/src/server/mod.rs
crates/devup-mcp/src/server/navigation.rs
crates/devup-mcp/src/server/project_context.rs
crates/devup-mcp/src/server/project_context_export.rs
crates/devup-mcp/src/server/projection.rs
crates/devup-mcp/src/server/review.rs
crates/devup-mcp/src/server/skills/devfive-frontend/SKILL.md
crates/devup-mcp/src/server/skills/fetch_tests.rs
crates/devup-mcp/src/server/skills/manifest.json
crates/devup-mcp/src/server/tools.rs
crates/devup-mcp/src/server/validation.rs
crates/devup-mcp/tests/ground_truth_tools.rs
docs/feedback-interactions/checks.json
docs/feedback-interactions/clippy.txt
docs/feedback-interactions/fmt-check.txt
docs/feedback-interactions/integration-tests.txt
docs/feedback-interactions/library-tests.txt
```

The report-only follow-up adds `docs/feedback-interactions/report.md`. The changepack marks `crates/devup-mcp/Cargo.toml` as Minor, following CI's required log convention; no versions were manually advanced. Cargo.lock only adds direct edges to already-existing workspace dependencies `oxc_ast_visit` and `quick-xml`.

Shared integration points are additive fields in `tools.rs`, thin routing/context plumbing in `mod.rs`, one interaction postprocessing hook in `projection.rs`, and the routes scope in `project_context.rs`. Projection comments are attached before output/file/resource hashing; original element source maps remain unchanged. No push, merge or release was performed. The coordinator should integrate the implementation and report commits alongside the companion theme/asset work, then reload the plugin and collect real Prime/browser evidence for the explicitly outstanding live checks.
