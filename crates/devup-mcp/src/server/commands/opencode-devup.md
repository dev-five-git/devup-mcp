---
description: Plan and deliver DevFive projects, features, fixes, Figma pages and refactors with project intake, ERD models and supervisor work units
---
<!-- devup-mcp:command devup - written by devup_skills install; rewritten by later installs. -->

Run the devup workflow for: $ARGUMENTS

The arguments are optional Figma links, Jira keys, spec URLs/files and a goal.
For Figma work, one attached Devup Bridge plugin supplies the selection when
a link is omitted.

Follow the `devup` skill step by step - it is installed next to this command -
and the devup-implementation-plan skill for its rules:

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


For Figma work, continue with the following branch after project intake:

1. `devup_figma_auth` status.
2. `devup_skills` status, then install what is missing, and load the skills.
3. `devup_project_context` with scope theme and scope routes.
4. `devup_screen_plan` draft with projectRoot, the link, the notes and
   extraQuestions for anything you would otherwise guess later.
5. Ask every question now with the `question` tool: the flat `questions` list
   in one call. Never in prose. New user-owned decisions after confirm still
   use the question tool; workers escalate to the supervisor.
6. `devup_screen_plan` confirm with every answer that applies.
7. Latest first, without asking: at the repository root run
   `bunx @dependency-check-updates/cli -d -u --rm` - it updates every
   manifest in the repository, every bun workspace and Cargo.toml included,
   and --rm removes the lockfiles and installed directories so everything
   resolves fresh - then `bun install`, build, lint and typecheck; fix code
   to the new APIs, pin only when no fix exists, and record it with
   `devup_screen_plan` assume (dependencyUpdate). Check current docs before
   using any library API.
8. Implement URL by URL: `devup_figma_export` with
   projectRoot and frameIds, plus the plan's defaults - each element's role
   recipe (hover, pressed, focus ring, current page, expanded, disabled,
   loading; restrained), one shared transition, reduced motion, view
   transitions by the plan's mechanism, metadata with openGraph, lang, alt
   text, image sizes, a not-found page. Validate with `devup_ui_validate`
   and projectRoot. Record anything new with `devup_screen_plan` assume.
9. `devup_screen_plan` auditPlan with baseUrl and every export artifactId:
   one audit plan per URL - health and overflow at every width; page,
   sections and assets against their Figma renders, and their size and
   position exactly (0.5px rounding, no tolerance), at the drawn widths;
   merged pages per width; dynamic routes at concrete URLs; hover, focus; a
   view-transition check last.
   Export what missingReferences names and run auditPlan again.
10. URL by URL: run the plan (a `devup_runtime_audit` plan, with the runner
    auditPlan wrote), `devup_screen_plan` status with auditPlanPath
    and auditReportPath, run every visualComparisons call and pass the
    results as visualResults, fix each finding by its class and fix hint in
    workQueue order, re-run. A geometry mismatch is code-drift (restore the
    export) or converter-bug (repro filed, never retried). After
    maxFixAttempts (default 3) a finding is unresolved with its evidence. No runner: toolingUnavailable, carry on.
11. Continue independent work: an ordinary choice mid-run goes to `devup_screen_plan` defer
    with the recommended option; take it with assume at the end (a
    destructive one stays unbuilt).
12. Done = zero fails at the drawn widths and no health or overflow fail at
    any width, or a filed converter bug, or listed as unresolved with
    evidence. Also every URL implemented, no data-figma-node-id in the
    production build (strip plugin in vite.config, grep the output), `bun run build`, `bun run lint`, `bunx oxlint
    --deny-warnings`, `bun tsc --noEmit` in each changed app and `bun test`
    pass (`bun run test` adds cargo tarpaulin, which does not work on
    Windows), devup_ui_validate has no errors, site completeness filled.
13. Finish with the status finalReport and summary.

User-owned decisions, before or after confirm, use the runtime question tool.
Show the evidence table first. Keep table approval and individual decisions as
separate entries in the same call, with headers up to 30 characters and two to
four options, recommended first and labelled "(Recommended)". Never ask for
numbers in chat. Record the chosen answer, decision id, affected model/endpoint/file
and rationale with existing defer/assume actions, not an invented ask action.
Update source models and re-confirm ERD changes before dependent implementation.
Without a question tool, report the limitation and leave that decision unresolved.
