# Using devup-mcp

## Project intake comes first

`/devup` is the entry point for new projects, existing features, bug fixes,
Figma page implementation and refactors. Figma screen planning is a branch
of the confirmed project plan; complete this section before the screen workflow below.

Read sources before asking: `devup_project_context` for every supported scope,
Atlassian MCP `getJiraIssue` / `searchJiraIssuesUsingJql` for issues, epic children
and dependencies, referenced Confluence/spec documents, and Figma exploration
(screen names, field labels, forms, tables and detail screens). Pass each read as
`evidence: [{source, facts}]` to `devup_project_plan`. Facts use `goal`, `parts`,
`figma` (links), `screens` ([{name, fields}]), and `<featureId>.screens`, `.states`,
`.permissions`, `.forms`, `.notifications`. Backend decisions use `erd`, `tenancy`,
`auth`, `lifecycle`, `indexes`, `storage`; delivery uses `jiraPolicy`, `branchPolicy`,
`prPolicy`. A sourced fact is a default with its provenance, not another question.
Sources are read by the runtime's connectors, never by treating a URL as its contents.

1. `devup_project_plan intake` with projectRoot: always ask the four initial
   questions with the runtime question tool (recommended option first): work type
   (refactor may be a custom answer), optional Figma/Jira/spec sources, one-sentence
   goal, and parts. Existing `apis/` recommends front + admin + API;
   `apps/admin` recommends front + admin. Retain planId on every later call.
2. Read newly named sources. `draft` with all initial answers, source-labelled
   evidence and `features: [{id,scope,screens,acceptance}]` or
   `issues: [{key,scope,dependencies,acceptance}]` (epic children are issues too).
   Each unresolved feature gets concrete screen/role, state, permission,
   validation, notification/integration rounds. Put all remaining unknowns into
   extraQuestions. No implementation starts while a question remains unanswered.
3. API in scope: inspect existing `vespertide.json` and `apis/*/models/*.json`.
   Draft complete `entities` as vespertide model objects and `relations` with
   from/to, cardinality oneToMany|manyToMany, column/joinTable and onDelete.
   Signup/post Figma evidence seeds user/post entities and a user FK, but the
   agent must expand every actual domain from the source evidence. Review the
   ERD round, ownership/tenancy, auth with integer roles (0,10,50,100), status
   enums, soft delete, audit timestamps, nullable fields, unique/composite keys,
   filter/sort indexes, file/image storage and cascade/restrict/set_null actions.
   Read https://github.com/dev-five-git/vespertide/blob/main/SKILL.md and schemas/.
   Use snake_case, `$schema` on every model, inline PK/FK/unique/index constraints,
   and table constraints only for CHECK. A manyToMany relation generates its join
   table. modelsDir is relative to projectRoot; set it explicitly for multiple APIs.
   Re-draft structured entities for requested corrections before confirming.
4. When Figma is present, run the existing screen-plan draft/rounds/confirm
   workflow (routes, groups, exclusions, site facts) before project confirm.
5. Ask all delivery questions now: Jira none/comment/transition, branch, PR.
   `devup_project_plan confirm` requires every applicable answer and writes source
   models plus confirmed state in one transaction. Then in modelsWorkingDirectory
   run the returned commands, in order: `vespertide diff`, `vespertide sql`,
   `vespertide revision -m "confirmed project plan"`, `vespertide export --orm seaorm`.
   The tool returns commands; the runtime executes and checks them. Never hand-write
   migrations/ or src/models/. Build vespera CRUD and feature routes, openapi.json,
   and the devup-api client; `devup_stack_diff` must show zero drift before done.
6. Latest-first updates, vinext, exact layout, all interaction/site defaults,
   runtime/visual audits and the existing never-wait rules below still apply.
   After confirm, use project-plan defer with id/reason/recommended/destructive;
   continue independent work. At the end assume with no decisions resolves all
   pending recommendations; destructive decisions remain unbuilt. Status includes
   ERD Mermaid, decisions and the unit table; retain unresolved evidence in reports.

## Supervisor execution

`devup_project_plan split` splits work, not files. Jira input produces one unit
per issue/sub-task with its explicit dependencies (reject cycles/unknown IDs).
Without Jira, it produces shared layout/theme first, then each feature's
models -> API -> client -> front chain; front-only features depend on shared.
Review cross-feature dependencies before dispatch; the supervisor sequences
shared database work to avoid concurrent schema edits. Each unit has acceptance
criteria and build/lint/typecheck/tests/validate/runtime/visual/stack-diff checks.
`.devup/units/<id>.md` contains scope, decisions, ERD and screen context, and checklist.

Default `unitQuestions: "supervisor"` means workers never ask the user: questions
were settled across all units during intake, and supervisor ownership avoids
repeated questions and keeps decisions consistent. `unitQuestions: "worker"`
is an explicit alternative, still subject to the confirmed never-wait policy.
Load `orca-cli` and `orchestration` skills to create one worktree per unit,
dispatch the DAG in parallel where independent, collect worker_done, and rerun
each unit's checks yourself before merging. New decisions use orchestration
ask/reply to the supervisor: answer from recorded decisions, or defer with a
recommended option while the worker continues other work. Send failed checks
back to the same worker session. Update unit states with status unitUpdates
(pending -> running -> verifying -> merged, or unresolved); merged needs recorded
supervisor verification. Without Orca, execute the same units sequentially in one
session. Only under the recorded Jira/PR policy, comment results and PR link and
transition issues. Final report includes the unit table, DAG, ERD and unresolved
items; never claim completion from split or command generation alone.



These are the rules that used to be pushed to every client in `initialize`.
They are published here instead so a caller that never converts a Figma design
never carries them. Rule numbers match the original list, which skipped 9.

## 1a. Connection order: bridge -> direct

Two paths reach Figma and they are not equals. The Devup Bridge plugin, run in
the Figma desktop app on the file, needs no login and spends none of Figma's
allowance. The direct path is OAuth against Figma's MCP and is metered. Use the
bridge first: it serves every read a collection makes, and direct only re-reads
what the plugin answers short - data Figma withholds while the file is out of
sight, a reference PNG the plugin cannot draw as get_screenshot does, a read an
older plugin does not know. `source.directFallbacks` counts those re-reads.

- Call `devup_figma_auth` with `action: "status"` before asking anyone to log
  in. It reports both paths: `connected`, `activePath` (`bridge`, `direct` or
  null), `paths.bridge.attachedFiles` with each plugin's file name, current
  page and selection, and a `nextAction`. `status` reads `disconnected` only
  when neither path can serve. Do not ask for a login while `activePath` is
  `bridge`.
- With exactly one plugin attached, `devup_figma_export`,
  `devup_figma_search` and `devup_figma_explore` take no `url`. The export
  reads the file that plugin has open and the node selected in Figma;
  `frameIds` without `url` name frames in that file - one id is that frame,
  several are screens of the selected Section. Explore anchors on the
  selection, and search reads the whole file.
- When a link is still needed, use `figma-bridge://current`, with
  `?node-id=1-2` to name a node. Never invent a Figma file key to fill `url`:
  a plugin that cannot report its key answers any key while it is alone, so an
  invented one is served and then mistaken for the file's.
- With no plugin attached, or several, a call without `url` is refused with a
  `nextAction` that lists the ways forward: run the plugin, log in and pass a
  link, or pick one of the attached files.
- An answer the plugin served says so: `source.kind` is `bridge`, with
  `bridgePort`, `fileName`, `pageName` and `bridgeReads`. `source.fileKey` is
  the key the plugin reported, or `null` with `fileKeyReason` when it could
  not report one.

## 2a. Ask for an output only when you will read it

Measured on the Korean WQUW-120 modal, semantic sourceMap is about 5.4x TSX
(48,832 versus 9,099 UTF-8 bytes; 12.6% smaller than its former offset map).
Sizes vary by screen; earlier rawPayload/rawSnapshot measurements were about
7x/2x TSX, so requesting them by default spends most of the response on bytes
nothing reads.

sourceMap records nodeId, original property, generatedProperty and resolution,
plus optional variableId (variable token), styleId (style token) and assetId
(asset reference), with no character/byte offsets. Each entry's elementIndex,
which joins it to `devup_ui_validate` violations, adds 11-14% to the map
(measured on the ten WQUW-151 screens).

Resolution labels carry different weight. `raw-fallback` means a raw-value
mapping, not necessarily an inaccurate value. `verified-explicit-dimension`
checks that emitted pixels equal the source dimension, while
`verified-layout-sizing` checks sizing intent against emitted CSS. `exact`
verifies that field-to-property mapping, not rendered pixel equivalence.

Use the `generatedSource` diagnostics for node code excerpts. `rawSnapshot` and
`rawPayload` are for banking a capture as an offline fixture, and both require
`debug: true`.

`componentTsx` is the same screen with instances left as `<Name />` references,
and `responsiveTsx` appears on its own whenever the capture carries more than
one width.

## 3. Screenshots and visual reasoning verify; they do not author

`get_design_context`, screenshots and visual reasoning are verification aids
only. Do not overwrite devup-mcp output with what a picture looked like.

## 4. Do not hand-interpret the design

Do not read a node tree yourself to write devup-ui code, and do not infer
layout from coordinates.

## 5. A failed call is a fact to report

If a devup-mcp call fails, record it explicitly. Do not silently route around
it.

## 6. Never guess a UI value

Do not guess color, spacing, radius or typography. If you could not obtain a
value, stop and report that.

## 7. A SECTION link is two steps, not one subtree

Do not implement a Section link as one whole subtree. Read the
`selection_required` candidates and continue with bounded per-screen `frameIds`
batches from `nextAction`. `allScreens` is valid only when the complete list
fits the advertised frame and output budget.

## 8. The generated component name is a starting point

The name comes from the Figma layer name and is not a contract. Rename it to
fit the codebase, and rename any name that is meaningless or not a valid
identifier.

## 10. An asset path in the output is a placeholder

A `maskImage` or `Image` src is built from the layer name. Rename the file to
fit the project. If the asset varies per usage, lift it into a prop instead of
hardcoding it.

`maskImage` + `bg` is emitted only for an icon painted in one flat colour; a
multi-colour icon (gradient, several colours, an image) stays `<Image src>`.
Never turn an `<Image>` icon into a mask or recolour it by hand: a mask keeps
only the shape and fills it with one colour. `devup_ui_validate` with
`projectRoot` warns `multicolor-mask` when one does.

## 11. A referenced asset must actually exist

A fixed asset such as an icon must be exported, never referenced by a path that
does not exist yet. Read `assetManifest` for the asset IDs, then call
`devup_figma_export` again with `assetRequests`, giving each entry an
`outputPath` under an allowed write root, and make the path in the code match
the path you wrote.

Image paints default to `renderMode:"original-source"`: acquire the complete source,
apply crop, blur, rounding and blend in CSS, and inspect `effectsBaked` and
`effectsAppliedBy`. Original scale never upscales the source. `node` explicitly
renders that node at scale 2 by default; fractional 1.5 is supported and 1.5–3 is
recommended. Low-resolution requests remain accepted with explicit warnings.
`visible` exports a selected acquired ancestor subtree, including that scope's
clips/masks/rounding. Supply `visibleScopeNodeId`; outside-scope composition stays
unknown. `nodeId` selects any acquired in-scope node, including parent instances
that asset discovery did not classify. Keep usable SVG vectors as SVG.
Legacy `fill` retains matching PNG/JPG bytes at scale 1 and no resizing; explicit
WebP fill conversion is labelled transcoded. Full-source codec conversions are
also labelled: a JPEG-to-PNG conversion does not preserve original byte identity.

For web images request `format:"webp"`, integer `quality:0..100` (default 85),
with alpha preserved and optional lossless encoding. Optional `maxWidth` caps
original-source WebP width without upscaling. `srcset:true` emits a base image and
an available higher-resolution `@2x.webp` derivative; descriptors report actual
encoded density, including caps. Read sourcePixelSize, outputSize, nodeBox,
source/output effective scales, savings against source and a real PNG baseline.
Conversion is bounded to 8192 per side, 16777216 pixels, 128 MiB decode allocation
and 1 MiB inflated compressed PNG metadata. Animated/unsupported conversion is
refused rather than silently flattened; metadata is not preserved.

`preset:"web-ready-images"` acquires every in-scope IMAGE paint as full-source
WebP, quality 85, capped at display width ×2 when known, with CSS effects. It adds
assetManifest/imageAudit and `asset-css-map.json`, schedules sorted batches of at
most six and retains hidden, missing-source, failed and limit dispositions.
Use assetPublicRoot for written `.webp` files and mapped code, or resources for
reusable bytes. Custom assetRequests remain separate. Poll exportJob; resume
paused work or cancel with jobAction. Polls return compact asset rows; add
`responseFields` (e.g. `exportJob.assets`) to a poll for full rows. `assetJob`
only references `exportJob`. Accepted results/checkpoints live in this
server process for the reported retention period. Scheduling is bounded to4096
sources and128 MiB retained asset bytes; remaining IDs carry explicit failures
and can be retried in smaller scopes. Read completeness and failures before use.

## 12. Prefer resource delivery for assets and large outputs

With `delivery: "resource"`, devup-mcp returns `devup://artifact/...` resource
links to read on demand instead of inlining bytes into every response.

## 13. Exact means exact at the drawn width

`quality.projection` grades the mapping at the width the frame was drawn at,
not at any other viewport. Before widening a generated screen, read
`widthBehavior`: a finding marked `consumer-policy-required` is a question the
design does not answer. Choose an anchor from its `candidates` and pass it in
`widthPolicy` (`{"root": "fluid", "anchors": {"<nodeId>": "right"}}`) rather
than re-anchoring the generated code by hand. How the screen renders at other
widths is checked in the consumer project: `devup_runtime_audit` writes the
plan and verifies the report, and never runs the browser itself. Its ingest
returns a ready `devup_visual_compare` call for each screenshot taken at a
drawn width.

## 14. Interactions by role, restrained

Hover plus a transition is not a finished interaction. Each interactive
element gets its role's recipe - button, link, card, nav item, icon button,
input, disclosure - with pressed, focus, current-page, expanded, disabled and
loading states where they apply, and view transitions between pages - always
on, and proved by the runtime audit's view-transition check. Restraint is a rule, not a taste: at
most two properties per state, 150-250ms, existing tokens only, scale 0.97 to
1.05, nothing infinite, everything off under `_motionReduce`. The table is in
the devup-implementation-plan skill; `devup_screen_plan` records it in
`defaults` and `devup_ui_validate` reports both too little and too much.

## 15. The latest stack, and vinext

DevFive builds on Next App Router run by vinext on Vite, in bun workspaces,
on the latest versions. An app with a `vinext` dependency, `vinext` scripts or
a `vite.config` importing vinext is an App Router app with no `next`
dependency; `devup_project_context` reports `frameworks` per app. Before
implementing, the workflow runs `bunx @dependency-check-updates/cli -d -u --rm`
at the repository root, then `bun install`, build, lint and typecheck, and
checks current docs before using a library API - API knowledge from memory is
assumed stale. A new project starts from `dev-five-git/service-template`; one
on the `next` CLI is asked once whether to migrate.

## 16. Visual verification of everything, full autonomy

Visually verify every screen, section and asset at every drawn width.
`devup_screen_plan` `auditPlan` writes one runtime-audit plan per URL: page
health, overflow and clipped text at every width; the page, each section and
each asset against its Figma render, and the size and position of each
section, card and asset, at the drawn widths; hover, focus and a view
transition, ordered by code. Layout is exact; a mismatch is drift or a
converter bug: geometry allows only half a pixel of rounding, and a
mismatch is `code-drift` when the code no longer writes what the export
recorded in `.devup/exports/`, or `converter-bug` when it does - reported
with a repro in `.devup/converter-bugs/`, never retried or tuned silently.
Merged pages are found per width through `data-figma-node-ids`, dynamic
routes at concrete URLs, and production builds strip the ids unless
`DEVUP_AUDIT=1`. `status` classifies each failure (runtime-error,
overflow, layout-position, layout-size, asset-mismatch, section-visual,
page-visual, interaction, view-transition) with its node, file and fix hint.
Never stop to wait; reorder the work: a decision after confirm goes to
`defer` and is taken with `assume` at the end. Done = every required URL x width cell passes. Report unresolved findings
with evidence after `maxFixAttempts` as incomplete.

Before reporting done, run the runtime audit for every confirmed URL at every required width (default 1440/1100/858/390), read the report, fix failures and rerun until every cell passes. Missing cells are not-run and block completion. At drawn widths require rendered completeness and page, section and asset visual comparisons. Check sequence-order and section-layout at every width.
Ask all site, structure and URL questions from the draft questions/questionRounds together up front, back to back before implementing. Never ask another round during the build; record conservative assumptions after confirmation.
