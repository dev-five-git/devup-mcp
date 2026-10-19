---
name: devup-implementation-plan
description: |
  Begin DevFive projects, features, bug fixes and refactors with project intake,
  confirmed ERD models and supervisor work units. For Figma, plan which screen
  becomes which URL, confirm it with the user in one
  batch of questions, then implement URL by URL and finish with a URL x width
  status table. Uses devup-mcp's devup_screen_plan, devup_figma_export and
  devup_runtime_audit.

  TRIGGER WHEN:
  - Starting a DevFive project, feature, bug fix or refactor through /devup
  - Asked to implement, build or convert a Figma design, Section or several
    screens into pages
  - Given a Figma Section link, or several frames, for a Next App Router project
  - Asked what was built from a design, where it lives, or whether it works

  Start it even when nobody typed /devup: a request to build pages or a site
  from Figma is the trigger. /devup only makes it explicit.

  Trigger keywords: implement this design, Figma to pages, build the site,
  screens, routes, URL, screen plan, devup_screen_plan, /devup
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



# Screen plan first, confirm once, then implement

A design handed over as "implement this" is several screens, and each one has
to land on a URL. If you pick those URLs yourself and start writing pages, the
user cannot tell afterwards what was built, where it lives, or whether it
works. On one real site the pages were written, some widths showed "This page
couldn't load", narrow widths were cut off on the right, and nobody had a
URL-by-width table on which to notice.

So the order is fixed, and none of it is optional.

## Hard rules

- **No implementation before confirm.** No page, layout or component file is
  written until `devup_screen_plan` `confirm` has succeeded.
- **All questions up front, back to back.** Ask every question before any
  implementation starts. If your question tool has no per-call cap (OpenCode
  `question`), ask the flat `questions` list in one call; each conditional
  question says when it applies. If it has one (Claude Code AskUserQuestion:
  4), ask `questionRounds` in consecutive turns with no other work in between;
  pass `maxQuestionsPerRound` to draft when your cap is different. Never one
  decision at a time spread through the work.
- **Ask what the whole job needs, now.** Every question you would need at
  any point - screens and routes, site files and facts, and your own
  (`extraQuestions`) - goes into the up-front rounds. Then finish the site
  without stopping.
- **Never ask during implementation.** After `confirm`, ask the user nothing:
  someone may have walked away from the keyboard, and a question three hours
  in stops everything. When something new comes up, take the plan's
  recommended or the most conservative option (leaving a frame unbuilt is
  conservative; inventing a URL is not), record it with `devup_screen_plan`
  `action: "assume"`, and keep going.
- **Ask with the question tool, never in prose.** Pass each round's
  `questions` to your runtime's structured question tool (`question` in
  OpenCode, `AskUserQuestion` in Claude Code, whatever yours is called). They
  are already shaped for it - `id`, `header` (30 characters or fewer),
  `question`, `options[{label, description}]` with the recommended option first
  and marked " (Recommended)", and `multiple`. Custom answers are allowed.
- **Skip what does not apply.** A question with `when` is asked only if the
  named question got that answer, e.g. a per-frame route only after "Separate
  pages". Rounds are ordered so the answer it depends on is always already in
  hand.
- **Stay inside the scope the user gave.** Only frames under the linked
  Section, frame or page (or the Bridge selection) are candidates. Never widen
  to the whole file or other pages on your own, and never build from trash,
  archive, old, draft, test, copy or hidden areas: draft lists them under
  `excluded` and asks once whether to accept that.
- **Never resolve a cross-section ambiguity yourself.** Frames that look like
  one screen in different sections - device-named ones such as PC / Mobile
  included - are a designer's structure. Ask the `sections:` question as
  returned; it names the sections, frames, widths and node ids.
- **Frames marked for a design fix are reported, not built.** When the user
  answers "The Figma structure is wrong: I will fix it", those frames are left
  out, listed under "Design fixes requested" with what to move or rename, and
  the rest of the work goes on without waiting.
- **The stack is DevFive's default.** Next App Router run by vinext, bun
  workspaces, devup-ui, oxlint, the latest versions. A project with no app
  starts from `dev-five-git/service-template` (a default, not a question); one
  on the `next` CLI is asked once, up front, whether to migrate to vinext -
  never migrated silently.
- **Pick the app by meaning, not by name order.** In a repository with
  several apps, `apps/admin` is the admin console and a Figma design for a
  public site is almost never for it. The plan decides from the app's name
  (`front`, `web`, `site`, `www`, `landing`, `home` for the public site;
  `admin`, `dashboard`, `console`, `backoffice`, `cms`, `manager`, `관리자` for
  the console), its routes (sign-in, dashboard) and the frames' own names
  (Home, About, 회사소개 vs Admin, 관리자, 회원관리, 대시보드). When the
  evidence points one way it takes that app without asking and records why in
  `defaults`. The bias is the public app unless there is clear admin
  evidence: a file with both maps admin screens to the console and every other
  screen to the public app (the route table's `app` column), and frames that
  say nothing either way go to the public app too. Only when no app looks like
  the public site is the app asked.
- **Never invent a route.** A route comes from an existing page built from the
  frame, a nav link already bound to an href, an explicit `navigationTargets`
  entry, the project's `routeAliases`, or the user's answer. A route inferred
  from a layer name is `routeEvidence: "name-match"` and is put to the user.
  `proposedRoute: null` with `routeEvidence: "UNVERIFIED"` stays null until the
  user answers.

## Site completeness

A site built only from what Figma draws feels dead: no hover, no focus, no
pressed or current state, no transitions, no metadata, no icon. Hover plus a
transition is still not enough. The plan splits what a finished site takes in
two.

**Filled in by you, without a question** - draft lists these as `defaults`,
and the final report repeats them:

- the interaction recipe for each element's role (table below), on every
  interactive element `interactiveCandidates` and the navigation checks find;
- one shared `transition`, 150-250ms, ease or ease-out, on the properties that
  change, defined once (a token or shared constant), not per element;
- `_motionReduce` so `prefers-reduced-motion` gets no transform or animation;
- view transitions between pages, by the mechanism the plan chose from the
  project's versions (below);
- root `metadata` (title template, description, `metadataBase` when the URL is
  known) and per-page `title`, `description` and `openGraph` from the page's
  Figma heading and lead text - the devfive-frontend SEO rule; `lang` on
  `<html>`;
- `alt` from the layer name or text, width and height on every image, and a
  `not-found` page when there is none.

### Interaction recipe per role

| Role | Hover | Active / pressed | Focus | Also |
| --- | --- | --- | --- | --- |
| Primary button | one token step darker | `scale(0.98)` or one more step darker | 2px ring with offset (`_focusVisible`) | disabled: reduced opacity, `cursor: not-allowed`; loading (disabled + spinner) while the form it submits is pending |
| Secondary / ghost / outline button | subtle tinted background | slightly stronger tint | same ring | same disabled |
| Text link / "더 보기 >" | colour change or underline | - | ring | a trailing chevron moves 2px on hover |
| Card that links | 2-4px lift + shadow token; inner image zooms 1.02-1.04 (overflow hidden) | lift removed | ring on the card | - |
| Nav item / tab | colour | - | ring | current route: `aria-current="page"` and the Figma active style (e.g. the underline the header draws) |
| Icon button | background circle or tint | press | ring | `aria-label` required |
| Input / select / textarea | border | - | border + ring | invalid state with its message; disabled |
| Accordion / disclosure / dropdown | - | - | ring | `aria-expanded`; the chevron rotates 180deg |
| Mobile menu / drawer | - | - | - | an open/close transition, when the design has one |

**Restraint - hard rules.** Rich is not busy:

- Figma variant states first; otherwise existing theme tokens only. No new
  colours - record an assumption when nothing fits.
- At most two properties change per state.
- 150-250ms, ease or ease-out, one shared definition.
- No bounce, no infinite or attention-seeking animation; transforms of 4px or
  less and scale between 0.97 and 1.05.
- Everything that moves is off under `_motionReduce`.
- Non-interactive elements get no hover effect.

`status` counts every role against its recipe ("buttons 6/6
hover+active+focus, cards 4/6 hover+focus"); that table is the record of which
recipe each element got.

### View transitions

Always on. DevFive runs the latest versions - the dependency step comes
first - and the latest Next App Router and React support React's
`<ViewTransition>` on vinext and on the `next` CLI alike, so no version is
checked. View Transitions are a web standard: browsers without them simply
navigate.

- `import { ViewTransition } from 'react'`, wrapping each page's content in
  its `page.tsx` (not the layout, which persists across navigations).
- A short cross-fade (about 250ms), no big slides, `animation-duration: 0s`
  under `prefers-reduced-motion`, and `::view-transition { pointer-events:
  none }`.
- Name shared elements only where the same element is on both pages - a page
  title, a hero image, a card image and its detail page: a static name as the
  devup-ui `viewTransitionName` prop, a per-record name on `<ViewTransition
  name>` (devup-ui extracts style props at build time, so a runtime name
  cannot be a style prop).
- **Verify it, do not doubt it.** auditPlan gives each URL one
  `view-transition` check following its link to the next URL, placed after
  every other check (the executor also returns to the URL afterwards). It
  passes only when a view transition ran (`document.startViewTransition`
  called, or a cross-document `pagereveal` carrying one). A failure is a bug to
  fix and re-run, not an assumption. `experimental.viewTransition` goes into
  the next config only if that check fails without it (vinext's own example
  sets it).

Skipped only when `.devup/screen-plan.config.json` opts out
(`viewTransitions: false`), or `updateDependencies: false` leaves the project
on a React older than 19.3. If the project already animates pages with its own
library (framer-motion, next-view-transitions, ...), the plan asks instead.
The "motion beyond hover" question is about scroll reveal only.

**Asked, in the same up-front rounds** - only what needs a file or a fact:
the favicon and the share image (offered from existing files and Figma logo
or og nodes in scope; never generate or draw one), the site name and
production URL when the project does not say, analytics when the project
shows signs of it, scroll reveal ("None" unless the design shows motion), and
**your own questions**: pass `extraQuestions` to draft (same shape: `id`,
`header` of 30 characters or fewer, `question`, two to four `options` with the
recommended one first, `multiple`) for anything you would otherwise guess at
any point in the job - where a form submits, contact details, external links,
legal pages, a CMS or data source, the site's language. Collect every question
the whole job will need now; after confirm, never ask - use `assume`.

**Verified** - `devup_ui_validate` with `projectRoot` warns on interactive
elements with no focus style, on too much motion (over 400ms, scale above
1.05, infinite animation) and on icon buttons without `aria-label`, and
reports buttons without a pressed state, motion without `_motionReduce` and
nav links without `aria-current`; pages without metadata and an app without an
icon are warnings too. `status` reports site completeness item by item,
including a "view transitions: enabled (mechanism) / skipped (reason)" line.

## Steps

1. **List the screens.** If you only have a Section link, `devup_figma_explore`
   lists them; if you already hold the frame ids, you have the list. Then
   `devup_project_context` with `scope: "routes"` shows the existing pages.
2. **Draft the plan.** Call `devup_screen_plan` with `action: "draft"`,
   `projectRoot`, and the Section or frame `url` (or nothing, while one Devup
   Bridge plugin is attached and the Section is selected). Pass `screens` from
   step 1 (with each frame's `section`) to skip a second Figma read, and
   `navigationTargets` for any Figma node -> route binding you were given
   explicitly. It writes `.devup/screen-plan.json` and `.devup/screen-plan.md`.
3. **Show the plan, then ask everything.** Show the user the `screens` table
   (Figma screen, widths, proposed route, evidence, existing file, shared
   parts, open decisions), the `heuristics` list and the `excluded` list, then
   ask the questions - the flat list in one call, or the rounds back to back -
   exactly as returned, the site questions included. Do not reword the options; the labels are what confirm
   reads.
4. **Confirm.** Call `devup_screen_plan` with `action: "confirm"`, the
   `planId`, and `answers` keyed by question id (the chosen label, with or
   without " (Recommended)", or the user's custom text) for every question
   that applies. A refusal names the missing ids and returns them as rounds:
   ask those right away, still before any implementation.
5. **Latest first, before implementing, without asking.**
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
6. **Implement URL by URL, without asking.** For each confirmed route, call
   `devup_figma_export` with `projectRoot` and that screen's `frameIds`. With
   `pageScaffold`, omit `route`: the export takes it from the plan and refuses
   a different one. Add the plan's `defaults` (states, transition, reduced
   motion, metadata, lang, alt, image sizes, not-found) as you go, and run
   `devup_ui_validate` with `projectRoot` on what you wrote. Answer anything
   new yourself with the recommended or most conservative option and record it
   with `assume`. Screens confirmed as a state of another page are built
   inside that page. An export of a frame the confirmed plan does not place
   answers `plan_required` and writes nothing: leave it unbuilt and record an
   assumption - do not ask. An excluded or design-fix frame answers `refused`:
   it is never built, and no assumption changes that.
7. **Plan the audit.** `devup_screen_plan` `action: "auditPlan"` with
   `projectRoot`, `baseUrl` (default `http://localhost:3000`) and `artifacts`:
   every `devup_figma_export` artifactId you hold for the plan's frames. It
   writes one runtime-audit plan per URL under `.devup/runtime-audit/<slug>/`
   and the runner once under `.devup/runtime-audit/runner/`. Each plan checks,
   at every width plus each drawn width (its viewport as tall as the frame):
   page health (no page error, console error, error-boundary text, overflow
   or clipped text), and at the drawn widths the page against its
   referencePng, every section and asset against its own Figma render, and
   the size and position of every section, card and asset - **exactly**.
   Layout is converted statically, so at a drawn width it is either right or
   wrong: only half a pixel of rounding is allowed (Chromium measures in
   1/64 px, unsnapped at deviceScaleFactor 1, and Figma's fractional values
   are written rounded), and no tolerance can be set. The frames' fonts are
   loaded first, so text boxes are exact too; a missing font is an
   environment error. Between the drawn widths there is no design, so only
   health, overflow, containment, clipped text and the nav are checked. A
   page merged from one frame per width is found per width through
   `data-figma-node-ids` (`"1440:12:34 390:56:78"`), and an element a width
   does not draw is checked hidden there. A dynamic route (`[slug]`, `[id]`,
   catch-all) is audited at concrete URLs: its `generateStaticParams`
   (literal params, or the local data file it maps), else the links the
   audited pages hold into it (run once, then auditPlan again), else a sample
   from its Figma detail frame, recorded as an assumption - up to ten. Hover and focus
   sweep every link and button; one view-transition check follows a link to
   the next URL - placed last by code, so check order cannot break a run.
   `missingReferences` lists every referencePng, native render or frame
   export it lacks, with the export call that makes it: make them and run
   auditPlan again. Nothing is skipped silently.
8. **Run it, URL by URL.** In the runner, `npm ci`, `npx playwright install
   chromium`, then `node run.mjs --plan <planPath> --out <reportDirectory>`
   with the app running. If the runner cannot be installed, call status with
   `auditPlanPath` and `toolingUnavailable: "<why>"`: that URL's audit is
   inconclusive and the rest of the work goes on.
   This real-browser step is required before claiming any page is done: build
   and serve the actual generated app, not a hand-written stand-in. Static
   checks and TSX snapshots cannot detect missing background images, wrong
   colors or assets that never load. Run `image-loaded` and `page-health`, read
   the browser report, and retain screenshots with their viewport and reference
   identities. A report's `completed` means execution finished; a capture check's
   `pass` means only that pixels were captured. Neither is a visual verdict.
9. **Read the status, fix, re-run.** `devup_screen_plan` `action: "status"`
   with `auditPlanPath` and `auditReportPath`. Run every `visualComparisons`
   call (`devup_visual_compare`, each with its `threshold` and `diffPath`)
   and pass the results back as `visualResults` (`id`, `verdict`,
   `changedRatio`, `diffPath`). Every failure comes back as a finding with
   its class, Figma node, code file and fix hint:

   | class | what failed | first step |
   | --- | --- | --- |
   | `runtime-error` | the page threw, logged an error or showed an error page | read the first error; fix the component or the data state |
   | `overflow` | content wider than the viewport, or text cut off by its box | `maxW="100%"`, wrap or `minW={0}`; let the box grow |
   | `code-drift` | a box differs and its code no longer writes what the export wrote | restore the exported values; never hand-tune layout |
   | `converter-bug` | a box differs and its code still equals the export | a devup-mcp defect: its repro is in `.devup/converter-bugs/<node>.json`; file it. A local override needs a comment naming that file |
   | `layout-position` / `layout-size` | a box differs and there is no export record to compare | re-export with `projectRoot`, then the class is one of the two above |
   | `asset-mismatch` | an asset differs from its Figma render | a mask on a multi-colour SVG (use `<Image>`), the wrong file, crop or objectFit |
   | `section-visual` | a section differs from its render | the diff PNG's highlighted area: font, background, spacing |
   | `page-visual` | the page differs from its frame | fix sections first; then page background and section spacing |
   | `interaction` | hover or focus changes nothing | the role's recipe from the defaults, and `_focusVisible` |
   | `view-transition` | no transition ran on the navigation | `<ViewTransition>` around routed content, framework `Link` |

   Layout is exact; a mismatch is drift or a converter bug. Every export into
   the project records what it wrote per node in `.devup/exports/`, and
   status compares the element's code with it: a missing or changed exported
   prop, or a layout prop the export did not write, is drift; extra behaviour
   (handlers, hover styles, links) is not. A converter bug is reported in
   `converterBugs` and the final report and never retried - a deterministic
   result does not change.

   Fix every finding and re-run the URL, in the `workQueue` order. Pixels are
   not layout: anti-aliasing does not fail, comparisons allow 3% changed pixels for a page or
   section and 5% for an asset, and `.devup/screen-plan.config.json`
   `visualThresholds` tunes them (`page`, `section`, `asset`, or `nodes`
   keyed by node id for one asset) - raise one only for edge noise you have
   looked at in its diff, never to pass a wrong shape. After
   `maxFixAttempts` (default 3) fix-and-rerun rounds a finding is recorded as
   `unresolved` with its screenshot and diff, and leaves the queue.
10. **Never stop to wait; reorder the work.** A decision the work needs after
   confirm goes to `action: "defer"` (`decision: {id, about, question,
   options, recommended, destructive}`). Keep working: items it blocks move to
   the back of their URL's queue. When everything else is done, take each one
   with `assume` (`about`: the decision id, `decision`: the recommended
   option). A destructive one (deleting, overwriting, reaching outside the
   project) stays unbuilt and is listed in the report.
11. **Done means zero fails.** A URL is done only after its actual Chromium
   screenshots have been compared with independently exported Figma originals,
   every check and comparison passes at its drawn widths, and health and
   overflow pass at every width. Inspect the page/section/asset diffs for
   background images, colors and loaded assets; feed every result back to
   status. Missing references, unavailable browser tooling, not-run comparisons,
   converter bugs and unresolved findings remain incomplete with evidence.
   Preserve screenshots, reports, Figma reference identities and diffs.
   Before reporting done, also:

   - every confirmed URL is implemented;
   - the build passes: `bun run build` (`vinext build` for every app);
   - lint and typecheck pass: `bun run lint` (its prelint runs cargo clippy, fmt
     and check, then oxlint), `bunx oxlint --deny-warnings`, and `bun tsc
     --noEmit` inside each app you changed (TS 7); `bun test` passes. `bun run
     test` also runs `cargo tarpaulin` afterwards, which does not work on
     Windows - there, run `bun test` and `cargo test`;
   - `devup_ui_validate` reports no errors;
   - the production build carries no audit ids: each app's vite.config uses
     the `vite-plugin-strip-figma-node-ids.mjs` auditPlan put beside it
     (`plugins: [stripFigmaNodeIds(), vinext()]`), and after `bun run build`
     `grep -rl data-figma-node-id dist` finds nothing. The audit itself runs
     against `bun run dev` or an audit build, `DEVUP_AUDIT=1 bun run build`;
   - the site completeness block is filled.
12. **Finish with the final report.** End your reply with status's
   `finalReport`: the URL x width table (health, geometry, drift / converter
   bugs, page, section, asset, interaction, screenshot, diffs), the converter
   bugs with their repro files, the unresolved findings with their
   evidence, the pending decisions and how each was taken, the defaults, the
   design fixes requested, the excluded frames, the assumptions and the
   dependencies line - and its one-line `summary`. Report every `fail`,
   `not-run`, unresolved finding and assumption as what it is.

Hard rules, after confirm:

- Visually verify every screen, section and asset at every drawn width.
- Never stop to wait; reorder the work.
- Done = a real browser render and zero failed or missing comparisons;
  unresolved findings are reported as incomplete with evidence.
- Layout is exact; a mismatch is drift or a converter bug, never tuned.

## What the questions mean

| id | asked when | resolves to |
| --- | --- | --- |
| `scope` | the link itself points into a trash, archive or draft area | plan it anyway, or build nothing |
| `exclusions` | frames were excluded by name or because they are hidden | accept, or review each (`exclusion:` / `exclusion-route:`) |
| `group:<nodeId>` | one name drawn at several widths in one section | one responsive page, separate pages, or one route per frame (custom `/a, /b`) |
| `sections:<nodeId>` | one name in different sections | separate pages, one responsive page, or the Figma structure is wrong |
| `structure:<kind>:<nodeId>` | a duplicate, a screen split across sections, or a frame outside any section | keep as is, or the Figma structure is wrong |
| `split-route:<nodeId>` | `when` its group is separate pages | that frame's URL, skip, or not a page |
| `route:<nodeId>` | no evidence names a route, or evidence conflicts | the URL, skip, or not a page |
| `heuristics` | a route was inferred from a layer name | accept all, or review individually |
| `review:<nodeId>` | `when` heuristics is review | keep (merge or overwrite), another URL, or skip |
| `modal:<nodeId>` | the layer name looks like a modal or overlay | a state on a host page, or its own URL |
| `collision:<nodeId>` | an evidence-backed route already has a page | merge, overwrite or skip |
| `dynamic:<nodeId>` | the route has a `[segment]` | sample data, or wire the API now |
| `link:<nodeId>:<n>` | a link's destination is not in the plan | a route, or unlinked |
| `layout:<part>` | a part appears on several screens and no layout renders it | shared layout, or per page |

A `group` question recommends one responsive page: the widths are in one
section with matching names. A `sections` question recommends separate pages,
unless every section name is a device name (`PC`, `Mobile`, `Tablet`,
`모바일`) - and then it says the recommendation was inferred from those names.

## Project config

Designers name things differently, so a project can say how its own do it in
an optional `.devup/screen-plan.config.json`. Each entry present counts as
evidence and replaces the question it would otherwise take:

```json
{
  "modalKeywords": ["popup", "sheet"],
  "routeAliases": { "메인": "/", "회사소개": "/company" },
  "widthGrouping": "responsive",
  "excludeKeywords": ["trash", "archive", "legacy"],
  "includeKeywords": ["test drive"],
  "favicon": "app/icon.svg",
  "ogImage": "public/og.png",
  "siteName": "Prime",
  "siteUrl": "https://prime.example",
  "motion": "none",
  "framework": "vinext",
  "updateDependencies": true,
  "maxFixAttempts": 3,
  "visualThresholds": { "asset": 0.05 }
}
```

- `modalKeywords` replaces the built-in modal words.
- `routeAliases` maps a layer name to a route.
- `widthGrouping` is `responsive` (merge same-named widths in one section
  into one page), `separate` (never merge), or `ask` (the default: ask per
  group). Frames in different sections are always asked.
- `excludeKeywords` replaces the built-in words that mark an area as not for
  implementation (trash/휴지통, archive/아카이브, old/구버전/이전,
  deprecated/사용안함/미사용, backup/백업, draft/WIP/작업중, test/테스트,
  sandbox, playground, copy/사본).
- `includeKeywords` forces names containing them back in.
- `favicon`, `ogImage` (a path, or `figma:<nodeId>`), `siteName`, `siteUrl`
  and `motion` (`none` | `reveal`) pre-answer the site questions.
- `framework` (`vinext` | `next`) answers the migrate-to-vinext question,
  `updateDependencies: false` skips the dependency step for a project that
  forbids it, and `viewTransitions: false` opts out of view transitions.
- `figmaNodeIds: false` stops exports from writing `data-figma-node-id(s)`
  on the page, section, card and asset elements. The audit finds them by
  it, so keep it on. Production builds drop them through the Vite plugin
  auditPlan writes beside vite.config (`vite build` strips them unless
  `DEVUP_AUDIT=1`); on the `next` CLI use `compiler.reactRemoveProperties:
  process.env.DEVUP_AUDIT === '1' ? false : { properties:
  ['^data-figma-node-ids?$'] }`.
- `maxFixAttempts` (default 3) is how many fix-and-rerun rounds a finding gets
  before it is unresolved.
- Geometry has no tolerance to configure; it is exact.
- `visualThresholds` sets the changed-pixel ratio the comparisons allow:
  `{ "page": 0.03, "section": 0.03, "asset": 0.05, "nodes": { "12:34": 0.08 } }`.
