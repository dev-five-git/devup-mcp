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
   picks the one the frames are for by meaning - the public site for
   marketing screens, `apps/admin` only for admin screens.
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
9. **Runtime audit.** `devup_runtime_audit` `plan` with every confirmed URL at
   1440, 1100, 858 and 390 and one `view-transition` check (follow a link
   between two routes; it fails when no transition ran - a bug to fix), run
   the executor in the project, then `ingest`.
10. **Status.** `devup_screen_plan` with `action: "status"`, `auditPlanPath`
   and `auditReportPath`.
11. **Submission checklist.** Before reporting done, every item holds - fix
    and re-run until it does, without stopping to ask:

    - every confirmed URL is implemented;
    - the build passes: `bun run build` (`vinext build` for every app);
    - lint and typecheck pass: `bun run lint` (its prelint runs cargo clippy, fmt
      and check, then oxlint), `bunx oxlint --deny-warnings`, and `bun tsc
      --noEmit` inside each app you changed (TS 7); `bun test` passes. `bun run
      test` also runs `cargo tarpaulin` afterwards, which does not work on
      Windows - there, run `bun test` and `cargo test`;
    - `devup_ui_validate` reports no errors;
    - the runtime audit at every width shows no page error and no horizontal
      overflow, and its view-transition check passes;
    - the site completeness block is filled;
    - the final table `URL | Figma screen | file | status | widths verified |
      screenshots` is there, with the assumptions, the design fixes requested and
      the excluded frames.
12. **Final table.** End with the status `markdownTable` and `summary`: URL |
    Figma screen | file | status | widths verified | screenshots, then site
    completeness, design fixes requested, excluded frames, the defaults filled
    in and every assumption.
