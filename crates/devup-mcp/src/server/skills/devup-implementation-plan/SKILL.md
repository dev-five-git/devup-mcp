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

  Start it even when nobody typed /devup: a request to build pages or a site
  from Figma is the trigger. /devup only makes it explicit.

  Trigger keywords: implement this design, Figma to pages, build the site,
  screens, routes, URL, screen plan, devup_screen_plan, /devup
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
- **All questions up front, back to back.** Ask every question before any
  implementation starts. If your question tool has no per-call cap (OpenCode
  `question`), ask the flat `questions` list in one call; each conditional
  question says when it applies. If it has one (Claude Code AskUserQuestion:
  4), ask `questionRounds` in consecutive turns with no other work in between;
  pass `maxQuestionsPerRound` to draft when your cap is different. Never one
  decision at a time spread through the work.
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
- **Never invent a route.** A route comes from an existing page built from the
  frame, a nav link already bound to an href, an explicit `navigationTargets`
  entry, the project's `routeAliases`, or the user's answer. A route inferred
  from a layer name is `routeEvidence: "name-match"` and is put to the user.
  `proposedRoute: null` with `routeEvidence: "UNVERIFIED"` stays null until the
  user answers.

## Site completeness

A site built only from what Figma draws feels dead: no hover, no focus, no
transitions, no metadata, no icon. The plan splits what that takes in two.

**Filled in by you, without a question** - draft lists these as `defaults`,
and the final report repeats them:

- interaction states on every link, button, linked card, tab and input:
  `_hover`, `_focusVisible` (a visible focus ring), `_active`, `_disabled`
  where it applies, `cursor="pointer"`. Use the Figma hover/pressed/focus
  variants when they exist; otherwise a small, consistent fallback built only
  from existing theme tokens (a darker or lighter shade, an underline, a slight
  lift or shadow). Never invent a colour; with no suitable token, record an
  assumption;
- one shared `transition` of about 0.2s ease on the properties that change,
  defined once (a token or shared constant), not per element;
- `_motionReduce` so `prefers-reduced-motion` gets no transform or animation;
- root `metadata` (title template, description, `metadataBase` when the URL is
  known) and per-page `title`, `description` and `openGraph` from the page's
  Figma heading and lead text - the devfive-frontend SEO rule; `lang` on
  `<html>`;
- `alt` from the layer name or text, width and height on every image, and a
  `not-found` page when there is none.

**Asked, in the same up-front rounds** - only what needs a file or a fact:
the favicon and the share image (offered from existing files and Figma logo
or og nodes in scope; never generate or draw one), the site name and
production URL when the project does not say, analytics when the project
shows signs of it, and motion beyond hover ("None" unless the design shows
motion).

**Verified** - `devup_ui_validate` with `projectRoot` warns on interactive
elements with no `_hover`/`_focusVisible`, pages without metadata and an app
without an icon; `status` reports site completeness item by item.

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
5. **Implement URL by URL, without asking.** For each confirmed route, call
   `devup_figma_export` with `projectRoot` and that screen's `frameIds`. With
   `pageScaffold`, omit `route`: the export takes it from the plan and refuses
   a different one. Add the plan's `defaults` (states, transition, reduced
   motion, metadata, lang, alt, image sizes, not-found) as you go, and run
   `devup_ui_validate` with `projectRoot` on what you wrote. Screens confirmed as a state of another page are built
   inside that page. An export of a frame the confirmed plan does not place
   answers `plan_required` and writes nothing: leave it unbuilt and record an
   assumption - do not ask. An excluded or design-fix frame answers `refused`:
   it is never built, and no assumption changes that.
6. **Run the runtime audit.** `devup_runtime_audit` `plan` with every confirmed
   URL as a screen (`routePath`) at the default widths (1440, 1100, 858, 390),
   each with a `no-horizontal-overflow` check; run the executor in the
   project, then `ingest` the report.
7. **Read the status.** Call `devup_screen_plan` with `action: "status"`,
   `projectRoot`, `auditPlanPath` and `auditReportPath`. A width without an
   audit item is `not-run`, never `pass`.
8. **Finish with the table.** End your reply with the `markdownTable` from
   status - `URL | Figma screen | file | status | widths verified |
   screenshots`, followed by the site completeness block, the defaults filled
   in, the design fixes requested, the excluded frames and the assumptions
   made during implementation - and
   its one-line `summary`, such as
   `7 URLs: 5 implemented, 3 verified at all widths, /about overflows at 390`.
   Report every `fail`, `not-run` and assumption as what it is. Do not call the
   work done while a URL is not implemented or a width fails.

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
  "motion": "none"
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
