---
name: devup-implementation-plan
description: |
  Plan which Figma screen becomes which URL, confirm it with the user in one
  batch of questions, then implement URL by URL and finish with a URL x width
  status table. Uses devup-mcp's devup_screen_plan, devup_figma_export and
  devup_runtime_audit.

  TRIGGER WHEN:
  - Asked to implement, build or convert a Figma design, Section or several
    screens into pages
  - Given a Figma Section link, or several frames, for a Next App Router project
  - Asked what was built from a design, where it lives, or whether it works

  Trigger keywords: implement this design, Figma to pages, build the site,
  screens, routes, URL, screen plan, devup_screen_plan
---

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
- **Ask every question in one batch.** Never one at a time, never a second
  round of confirmations. Collect every open decision, then ask them all at
  once.
- **Ask with the question tool, never in prose.** Pass the `questions` array
  to your runtime's structured question tool in ONE call: `question` in
  OpenCode, `AskUserQuestion` in Claude Code, whatever yours is called. The
  array is already shaped for it - `id`, `header` (30 characters or fewer),
  `question`, `options[{label, description}]` with the recommended option first
  and marked " (Recommended)", and `multiple`. Custom answers are allowed. If
  your tool takes fewer questions per call than the array holds, say so and
  split only as far as the tool forces you to.
- **Never invent a route.** A route comes from an existing page, a nav link
  already bound to an href, an explicit `navigationTargets` entry, or the
  user's answer. `proposedRoute: null` with `routeEvidence: "UNVERIFIED"` stays
  null until the user answers.

## Steps

1. **List the screens.** If you only have a Section link, `devup_figma_explore`
   lists them; if you already hold the frame ids, you have the list. Then
   `devup_project_context` with `scope: "routes"` shows the existing pages.
2. **Draft the plan.** Call `devup_screen_plan` with `action: "draft"`,
   `projectRoot`, and the Section or frame `url` (or nothing, while one Devup
   Bridge plugin is attached and the Section is selected). Pass `screens` from
   step 1 to skip a second Figma read, and `navigationTargets` for any
   Figma node -> route binding you were given explicitly. It writes
   `.devup/screen-plan.json` and `.devup/screen-plan.md`.
3. **Show the plan, then ask.** Show the user the `screens` table (Figma
   screen, widths, proposed route, evidence, existing file, shared parts, open
   decisions), then pass `questions` to your question tool in ONE call, exactly
   as returned. Do not reword the options; the labels are what confirm reads.
4. **Confirm.** Call `devup_screen_plan` with `action: "confirm"`, the
   `planId`, and `answers` keyed by question id (the chosen label, with or
   without " (Recommended)", or the user's custom text). A refusal names the
   missing ids: that is your mistake, not a reason to ask again one by one -
   you already have the answers, or you skipped a question in step 3.
5. **Implement URL by URL.** For each confirmed route, call
   `devup_figma_export` with `projectRoot` and that screen's `frameIds`. With
   `pageScaffold`, omit `route`: the export takes it from the plan and refuses
   a different one. An export that would write a page without a confirmed plan
   answers `status: "plan_required"` and writes nothing - go back to step 2.
   Screens confirmed as a state of another page are built inside that page.
6. **Run the runtime audit.** `devup_runtime_audit` `plan` with every confirmed
   URL as a screen (`routePath`) at the default widths (1440, 1100, 858, 390),
   each with a `no-horizontal-overflow` check; run the executor in the
   project, then `ingest` the report.
7. **Read the status.** Call `devup_screen_plan` with `action: "status"`,
   `projectRoot`, `auditPlanPath` and `auditReportPath`. A width without an
   audit item is `not-run`, never `pass`.
8. **Finish with the table.** End your reply with the `markdownTable` from
   status - `URL | Figma screen | file | status | widths verified |
   screenshots` - and its one-line `summary`, such as
   `7 URLs: 5 implemented, 3 verified at all widths, /about overflows at 390`.
   Report every `fail` and `not-run` as what it is. Do not call the work done
   while a URL is not implemented or a width fails.

## What the questions mean

| id | asked when | resolves to |
| --- | --- | --- |
| `route:<nodeId>` | no evidence names a route, or evidence conflicts | the URL, skip, or not a page |
| `modal:<nodeId>` | the screen looks like a modal or overlay | a state on a host page, or its own URL |
| `collision:<nodeId>` | the route already has a page | merge, overwrite or skip |
| `dynamic:<nodeId>` | the route has a `[segment]` | sample data, or wire the API now |
| `link:<nodeId>:<n>` | a link's destination is not in the plan | a route, or unlinked |
| `layout:<part>` | a part appears on several screens and no layout renders it | shared layout, or per page |
| `responsive` | screens are drawn at several widths | one responsive route, or one per width |
