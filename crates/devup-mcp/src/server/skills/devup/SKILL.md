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
   plugin attached nothing needs a login; do not ask for one.
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
5. **Ask everything now, with your question tool.** The flat `questions` list
   in one call if your tool has no per-call cap; otherwise `questionRounds`
   back to back. Never in prose, never one decision at a time, never later.
6. **Confirm.** `devup_screen_plan` with `action: "confirm"`, the `planId` and
   every answer that applies.
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
8. **Implement URL by URL without asking anything.** `devup_figma_export` with
   `projectRoot` and each screen's `frameIds`; `pageScaffold` takes its route
   from the plan. Add the plan's `defaults` as you go: each interactive
   element's role recipe (hover, pressed, focus ring, current page, expanded,
   disabled, loading - restrained: at most two properties per state,
   150-250ms, existing tokens only), one shared transition, `_motionReduce`,
   view transitions by the plan's mechanism, root and per-page `metadata` with
   openGraph, `lang`, alt text, image dimensions, a not-found page. Run
   `devup_ui_validate` with `projectRoot` on what you wrote. Anything new:
   take the recommended or most conservative option and record it with
   `action: "assume"`.
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
11. **Never stop to wait.** A decision mid-run: `action: "defer"` with the
    recommended option, keep working, take it with `assume` at the end; a
    destructive one stays unbuilt.
12. **Done = zero fails.** Every check and comparison passes at the drawn
    widths and health and overflow pass at every width, or each remaining
    finding is a filed converter bug or unresolved with evidence. Also: every
    URL implemented, the production build free of `data-figma-node-id` (the
    strip plugin in vite.config; `grep -rl data-figma-node-id dist` finds
    nothing), `bun run build`, `bun run lint`, `bunx oxlint --deny-warnings`, `bun tsc --noEmit`
    per changed app, `bun test` (on Windows `bun test` and `cargo test`),
    `devup_ui_validate` without errors, site completeness filled.
13. **Final report.** End with status's `finalReport` and `summary`: URL x
    width (health, geometry, page, section, asset, interaction, screenshot,
    diffs), unresolved findings with evidence, pending decisions taken,
    assumptions, design fixes, exclusions, defaults and the dependencies line.

Hard rules: visually verify every screen, section and asset at every drawn
width; never stop to wait, reorder the work; done = zero fails, or listed as
unresolved with evidence; layout is exact, a mismatch is drift or a
converter bug.
