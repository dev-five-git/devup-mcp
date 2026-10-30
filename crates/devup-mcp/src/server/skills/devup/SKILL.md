---
name: devup
description: |
  Start any DevFive project, feature, bug fix, Figma page implementation or
  refactor with evidence-based project intake, all questions up front, ERD
  models, a confirmed plan and supervisor work units. The Figma branch retains
  screen planning, latest-first updates, vinext, exact layout, interaction states,
  metadata, runtime/visual audits and the final URL x width table.

  Invoke as /devup [sources] [goal]. Also use the Figma branch when asked to
  build pages or a site from a design, even without an explicit /devup.
---

## Project intake comes first

When replacing an inlined instance with a shared component, carry over every `instanceOverrides` entry through a new variant or prop, or explicitly report it as unresolved. Never discard `// @devup-override` annotations or the required `DEVUP_CODEGEN_INSTANCE_OVERRIDE` review item silently.

For a confirmed multi-width URL, `responsiveTsx` is the deliverable. Export all sibling frameIds in width order using screen-plan status's next export action. Never start from single-width `tsx`/`componentTsx` for its page or sections. Only wire links/events/data; never change generated layout values, collapse arrays, or add hard-coded sizes/layout wrappers. Review `generatedExport.generatedDrift` in status; its confidence-labelled findings are heuristic and missing mappings remain unverified.

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



# /devup [sources] [goal]

Arguments: $ARGUMENTS - optional Figma links, Jira keys, spec URLs/files and a
one-sentence goal. With one Devup Bridge plugin, its selection supplies the
Figma anchor when a Figma implementation has no link.

This is the devup-implementation-plan workflow, started explicitly. Follow it
in this order and do not skip a step. The devup-implementation-plan skill has
the full rules; these are the steps.

1. **Connection.** `devup_figma_auth` with `action: "status"`. With a Bridge
   plugin attached nothing needs a login; do not ask for one. For disconnected,
   out-of-view or mismatched-host states, load `figma-bridge-setup` and perform
   its computer-use steps yourself, then verify status and a small read.
2. **Skills.** `devup_skills` with `action: "status"` and this `projectRoot`;
   if anything is missing or outdated, `action: "install"`, then load the
   installed skills (devup-ui, devfive-frontend, devup-implementation-plan).
3. **Project context.** `devup_project_context` with `scope: "theme"` (the
   tokens the code must use) and `scope: "routes"` (the pages that exist, and
   `frameworks`: which app runs on vinext or next). No app yet: the plan
   starts from `dev-five-git/service-template`. With several apps the plan
   picks the one the frames are for by meaning - the public site unless the
   frames clearly say admin.
4. **Screen plan.** `devup_screen_plan` with `action: "draft"`, `projectRoot`,
   the link, the notes and `extraQuestions` - every question you would
   otherwise have to guess at any point in the job (form target, contact
   details, external links, legal pages, data source, language). It returns
   the screens, the excluded areas, the defaults it will fill in, and every
   open question - routes, sections and structure, the site questions
   (favicon, share image, site name and URL, analytics, scroll reveal) and
   yours.
5. **Show the proposal table, ask once.** Show draft's `proposalMarkdown`
   (every open decision with its recommended answer) in the language the user
   first wrote in, and ask ONE question with your question tool: accept, or
   name the rows to change. Collect known decisions in one question call.
6. **Confirm.** `devup_screen_plan` with `action: "confirm"`, the `planId`,
   `acceptRecommended: true` and only the changed rows in `answers`. If
   confirm reports two screens on one URL, retry with its
   `details.retryAnswers` merged into your answers.
7. **Latest first, before implementing, without asking.**
    a. At the repository root, run `bunx @dependency-check-updates/cli -d -u
       --rm`. It updates every manifest in the repository to the latest
       versions - every bun workspace and every Cargo.toml - in one recoverable
       batch (`--recover rollback` restores the originals), and `--rm` removes
       each lockfile and installed directory (`bun.lock`, `node_modules/`,
       `Cargo.lock`, `target/`) so the next install resolves everything fresh.
    b. `bun install`, then build, lint and typecheck. If the update breaks
       something, fix the code to the new APIs. Pin a version back only when
       no fix exists, and record the pin with its reason.
    c. Before using a library API, check its current docs (context7 or the
       library's site), not memory: what you remember is assumed stale.
    Record the outcome with `devup_screen_plan` `assume` (`dependencyUpdate:
    { bumped, pins }`); status shows it as the dependencies line. Skip only
    when `.devup/screen-plan.config.json` says `updateDependencies: false`.
8. **Extract components before pages.** The first implementation unit after
   confirm exports used `COMPONENT_SET`/`COMPONENT` nodes with
   `outputs: ["componentTsx"]` to `components/common` or `components/layout`.
   Record each `mainComponent` id -> generated file and artifact identity in
   `.devup/component-map.json`. Assemble pages by screen-plan section (a direct
   child of the screen frame root), starting from generated TSX and mapped imports.
   Never hand-write TSX from Figma nodes. Audit through the component/section
   file mapping and explicit rendered targets, not DOM Figma attributes.
   Unsupported section exports or instance overrides remain reported limitations.
   **Implement URL by URL.** `devup_figma_export` with
   `projectRoot` and each screen's `frameIds`; `pageScaffold` takes its route
   from the plan. Add the plan's `defaults` as you go: each interactive
   element's role recipe (hover, pressed, focus ring, current page, expanded,
   disabled, loading - restrained: at most two properties per state,
   150-250ms, existing tokens only), one shared transition, `_motionReduce`,
   view transitions by the plan's mechanism, root and per-page `metadata` with
   openGraph, `lang`, alt text, image dimensions, a not-found page. Run
    `devup_ui_validate` with `projectRoot` on what you wrote. Ordinary choices
    use `action: "assume"`; new user-owned decisions use the question tool.
9. **Audit plan.** `devup_screen_plan` with `action: "auditPlan"`,
   `baseUrl` and `artifacts` (every export artifactId of the plan's frames).
   One plan per URL: health, overflow and clipped text at every width; the
   page, each section and each asset against its Figma render and the size
   and position of each section, card and asset at the drawn widths -
   exact, half a pixel of rounding and no tolerance; merged pages per width
   (`data-figma-node-ids`); dynamic routes at concrete URLs; hover
   and focus; a view-transition check, last. Export whatever
   `missingReferences` names and run auditPlan again.
10. **Run, status, fix - URL by URL.** Run each plan with the runner it wrote,
   then `action: "status"` with `auditPlanPath` and `auditReportPath`. Run
   every `visualComparisons` call and pass the results back as
   `visualResults`. Fix each finding (its class, node, file and fix hint) in
   `workQueue` order and re-run. A geometry mismatch is `code-drift` (restore
   the export's values) or `converter-bug` (repro in `.devup/converter-bugs/`,
   reported, never retried). After `maxFixAttempts` (default 3) a finding
   is unresolved with its evidence. Runner cannot install:
   `toolingUnavailable`, and carry on.
11. **Continue independent work.** An ordinary choice mid-run: `action: "defer"` with the
    recommended option, keep working, take it with `assume` at the end; a
    destructive one stays unbuilt.
12. **Done = zero fails.** A real Chromium render is mandatory before claiming
    any page is done. Build and serve the generated app, execute the Playwright
    runner, read its report and compare the actual screenshots with independent
    Figma references. Static checks, TSX snapshots, a generated audit plan and a
    capture check's `pass` do not prove visual correctness. Verify background
    images, colors and loaded assets; inspect page/section/asset diffs and feed
    every visual result back to status. Missing references, unavailable browser
    tooling, failed assets and not-run comparisons mean incomplete. Preserve the
    screenshots, reference identity, report and diffs in the final evidence.
    Every check and comparison passes at the drawn
    widths and health and overflow pass at every required width. Report filed
    converter bugs and unresolved findings with evidence as incomplete. Also: every
    URL implemented, the production build free of `data-figma-node-id`.
DevupUI strips audit attributes by default; `DEVUP_AUDIT=1` keeps them.
Figma node attributes are off by default; opt in with `figmaNodeIds: true`
only where the export schema supports it. Do not change app build config for auditing
or register a strip plugin. Inspect the ordinary production output.
Run `bun run lint:fix` first, then `bunx oxlint --deny-warnings` at the root.
Never fix JSX prop order, import sorting or formatting errors by hand.
Fix only non-fixable leftovers by hand (type errors, logic and decisions the fixer cannot make).
**Formatter ownership:** this template has no standalone prettier config.
Formatting is owned by eslint-plugin-devup's `devup(prettier)` rule inside oxlint.
Do not run `bunx prettier --write` or biome: their defaults produce a different
style (semicolons and double quotes), undoing the project's lint formatting.
Then run `bun run build`, `bun run lint`, `bun tsc --noEmit`
    per changed app, `bun test` (on Windows `bun test` and `cargo test`),
    `devup_ui_validate` without errors, site completeness filled.
13. **Final report.** End with status's `finalReport` and `summary`: URL x
    width (health, geometry, page, section, asset, interaction, screenshot,
    diffs), unresolved findings with evidence, pending decisions taken,
    assumptions, design fixes, exclusions, defaults and the dependencies line.

Hard rules: visually verify every screen, section and asset at every drawn
width; never stop to wait, reorder the work; done = every required cell passes;
report unresolved cells with evidence as incomplete; layout is exact, a mismatch is drift or a
converter bug.

Run the runtime audit for every confirmed URL at every required width (default 1440/1100/858/390), read its report, fix and rerun until every cell passes. Missing cells are not-run and block done; require rendered completeness and page/section/asset comparisons at drawn widths. Ask known site, structure and URL decisions up front in one question call, not per-section rounds. New user-owned decisions during the build still use the question tool.

## Staged execution and shared work first

`devup_project_plan split` is staged, not one flat chain per feature:

1. Stage 0, supervisor alone: shared parts that repeat across routes (Header, Footer, a hero or sub-navigation that appears on many frames, navigation data), the `devup.json` tokens, one export of every shared asset, and the backend foundation (vespertide models, entities, migrations, `openapi.json`, client). Nothing else starts before it is merged.
2. Stage 1, parallel: one worker per top-level menu or Figma group. A feature that uses tables gets one `model-<table>` unit per table (shared by every feature that uses it), then api, client and front units. A feature with no tables gets only a front unit. Never run two units that edit the same table at once.
3. Stage 2, supervisor: front-to-API wiring check, `devup_stack_diff` with zero drift, runtime audit, visual compare.

Run only as many workers at once as there are independent groups, not as many as there are routes. Each brief must be usable on its own: the frames to export with their URLs, the exports already done, the shared components to import (path and props), what the worker must not change, and the checks to rerun. A worker that needs a shared component changed does not edit it; it reports a shared-change request (what, why, diff) to the supervisor, who applies it once and tells every unit that depends on it.

## ERD and endpoint contract before the front

Confirm the ERD together with the screens, before any page is written. From the Figma frames derive the data each screen needs: repeating cards or table rows (list endpoint with page, sort, category and query parameters), detail layouts (detail endpoint), forms (create endpoint and field rules), search boxes, filters, sorts and page numbers. Show the user one table of entities and columns, the endpoint contract per screen, and the Mermaid ERD, and confirm it once.

A screen is not finished while its data is a hard-coded array, a `TODO` or "replace when API exists" comment, a control with no handler, or a form that changes local state without a network call. If the user answers "wire to the API now", record the `operationId` and check that it exists in `openapi.json` before you call the screen done; if it does not, say the screen is unfinished instead of faking it. Anything left on a fixture must be recorded with `defer` and listed as unfinished in the final report. A public asset a page references that is missing from `public/` is a failure to fix by exporting it, not something to skip.

## Decisions go through the question tool

This rule takes precedence over "never ask during the build" and "never wait"
when a decision genuinely requires the user's authority, including after confirm.
Use OpenCode `question` or Claude Code `AskUserQuestion`, not a numbered chat
list ending with "tell me which numbers to change". Continue independent work
while the decision is pending; workers escalate to their supervisor, not the user.
Do not invent a project-plan `ask` action: use the runtime question tool and the
existing plan actions. Record the chosen answer, not an unaccepted recommendation,
with its decision id, affected model/endpoint/file and rationale. If it changes
the ERD, update the source models and re-confirm before dependent implementation.
If the runtime has no question tool, report that limitation and leave the
user-owned decision unresolved; a prose question or silent assumption is not a
substitute. Ordinary implementation choices still use defer/assume without asking.

Any decision that needs the user, before or after confirm, is asked with the runtime question tool, never as numbered prose such as "tell me which numbers to change". Show the evidence table first, then ask. Put the recommended option first, labelled "(Recommended)", with two or three concrete alternatives (for example "category as a kind column / as an enum column / as a free text column"). A header is at most 30 characters and a question has two to four options. Keep the confirmation of a table separate from the decisions inside it: the table is one approve-or-change question, each real decision gets its own entry in the same question call. Record each answer with `devup_project_plan defer` or `assume` (id, reason, recommended) so it survives in the plan; a decision that changes a model is applied to `models/*.json` and the next action says so.
