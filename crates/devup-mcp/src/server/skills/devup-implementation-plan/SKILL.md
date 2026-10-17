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
- **All questions up front, back to back.** Ask every round in
  `questionRounds` before any implementation starts, in consecutive turns with
  no other work in between. Several rounds are fine - AskUserQuestion takes at
  most four questions per call, and the server has already split them - but
  never one decision at a time spread through the work.
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
- **Never invent a route.** A route comes from an existing page built from the
  frame, a nav link already bound to an href, an explicit `navigationTargets`
  entry, the project's `routeAliases`, or the user's answer. A route inferred
  from a layer name is `routeEvidence: "name-match"` and is put to the user.
  `proposedRoute: null` with `routeEvidence: "UNVERIFIED"` stays null until the
  user answers.

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
3. **Show the plan, then ask every round.** Show the user the `screens` table
   (Figma screen, widths, proposed route, evidence, existing file, shared
   parts, open decisions) and the `heuristics` list, then ask round 1, round
   2, ... of `questionRounds` back to back, exactly as returned. Do not reword
   the options; the labels are what confirm reads.
4. **Confirm.** Call `devup_screen_plan` with `action: "confirm"`, the
   `planId`, and `answers` keyed by question id (the chosen label, with or
   without " (Recommended)", or the user's custom text) for every question
   that applies. A refusal names the missing ids and returns them as rounds:
   ask those right away, still before any implementation.
5. **Implement URL by URL, without asking.** For each confirmed route, call
   `devup_figma_export` with `projectRoot` and that screen's `frameIds`. With
   `pageScaffold`, omit `route`: the export takes it from the plan and refuses
   a different one. Screens confirmed as a state of another page are built
   inside that page. An export of a frame the confirmed plan does not place
   answers `plan_required` and writes nothing: leave it unbuilt and record an
   assumption - do not ask.
6. **Run the runtime audit.** `devup_runtime_audit` `plan` with every confirmed
   URL as a screen (`routePath`) at the default widths (1440, 1100, 858, 390),
   each with a `no-horizontal-overflow` check; run the executor in the
   project, then `ingest` the report.
7. **Read the status.** Call `devup_screen_plan` with `action: "status"`,
   `projectRoot`, `auditPlanPath` and `auditReportPath`. A width without an
   audit item is `not-run`, never `pass`.
8. **Finish with the table.** End your reply with the `markdownTable` from
   status - `URL | Figma screen | file | status | widths verified |
   screenshots`, followed by the assumptions made during implementation - and
   its one-line `summary`, such as
   `7 URLs: 5 implemented, 3 verified at all widths, /about overflows at 390`.
   Report every `fail`, `not-run` and assumption as what it is. Do not call the
   work done while a URL is not implemented or a width fails.

## What the questions mean

| id | asked when | resolves to |
| --- | --- | --- |
| `group:<nodeId>` | frames share a name across widths or sections | one responsive page, separate pages, or one route per frame (custom `/a, /b`) |
| `split-route:<nodeId>` | `when` its group is separate pages | that frame's URL, skip, or not a page |
| `route:<nodeId>` | no evidence names a route, or evidence conflicts | the URL, skip, or not a page |
| `heuristics` | a route was inferred from a layer name | accept all, or review individually |
| `review:<nodeId>` | `when` heuristics is review | keep (merge or overwrite), another URL, or skip |
| `modal:<nodeId>` | the layer name looks like a modal or overlay | a state on a host page, or its own URL |
| `collision:<nodeId>` | an evidence-backed route already has a page | merge, overwrite or skip |
| `dynamic:<nodeId>` | the route has a `[segment]` | sample data, or wire the API now |
| `link:<nodeId>:<n>` | a link's destination is not in the plan | a route, or unlinked |
| `layout:<part>` | a part appears on several screens and no layout renders it | shared layout, or per page |

Which way a `group` question recommends follows the evidence: several widths
in one section with matching names recommend one responsive page; the same
name in different sections recommends separate pages, unless the sections are
named only for devices (`PC`, `Mobile`).

## Project config

Designers name things differently, so a project can say how its own do it in
an optional `.devup/screen-plan.config.json`. Each entry present counts as
evidence and replaces the question it would otherwise take:

```json
{
  "modalKeywords": ["popup", "sheet"],
  "routeAliases": { "메인": "/", "회사소개": "/company" },
  "widthGrouping": "responsive"
}
```

- `modalKeywords` replaces the built-in modal words.
- `routeAliases` maps a layer name to a route.
- `widthGrouping` is `responsive` (merge same-named widths into one page),
  `separate` (never merge), or `ask` (the default: ask per group).
