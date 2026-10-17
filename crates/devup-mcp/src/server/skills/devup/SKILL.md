---
name: devup
description: |
  Build pages or a whole site from a Figma design with devup-mcp, end to end:
  connection, skills, project context, screen plan with every question asked
  up front, implementation with interaction states and metadata, runtime
  audit at every width, and a final URL x width table.

  Invoke as /devup [figma url] [notes]. Also use it, without being asked, when
  the user wants pages or a site implemented from Figma: a Section link,
  several frames, or "build the site/page".
---

# /devup [figma url] [notes]

Arguments: $ARGUMENTS - a Figma Section, frame or page link (optional while
one Devup Bridge plugin is attached; the Figma selection is used), then any
notes, which go into the plan's `notes` untouched.

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
