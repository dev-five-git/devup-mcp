---
description: Build pages or a site from Figma with devup-mcp (screen plan, questions up front, states, metadata, audit, final table)
---
<!-- devup-mcp:command devup - written by devup_skills install; rewritten by later installs. -->

Run the devup workflow for: $ARGUMENTS

The arguments are a Figma Section, frame or page link (optional while one
Devup Bridge plugin is attached) followed by notes for the plan.

Follow the `devup` skill step by step - it is installed next to this command -
and the devup-implementation-plan skill for its rules:

1. `devup_figma_auth` status.
2. `devup_skills` status, then install what is missing, and load the skills.
3. `devup_project_context` with scope theme and scope routes.
4. `devup_screen_plan` draft with projectRoot, the link, the notes and
   extraQuestions for anything you would otherwise guess later.
5. Ask every question now with the `question` tool: the flat `questions` list
   in one call. Never in prose, never during implementation.
6. `devup_screen_plan` confirm with every answer that applies.
7. Latest first, without asking: at the repository root run
   `bunx @dependency-check-updates/cli -d -u --rm` (-d: every manifest below
   the root, so apps/* too; -u: apply as one recoverable batch; --rm: remove
   lockfiles and installed directories next to each manifest, Cargo.lock and
   target/ included), then `bun install`, build, lint and typecheck; fix code
   to the new APIs, pin only when no fix exists, and record it with
   `devup_screen_plan` assume (dependencyUpdate). Check current docs before
   using any library API.
8. Implement URL by URL without asking anything: `devup_figma_export` with
   projectRoot and frameIds, plus the plan's defaults - each element's role
   recipe (hover, pressed, focus ring, current page, expanded, disabled,
   loading; restrained), one shared transition, reduced motion, view
   transitions by the plan's mechanism, metadata with openGraph, lang, alt
   text, image sizes, a not-found page. Validate with `devup_ui_validate`
   and projectRoot. Record anything new with `devup_screen_plan` assume.
9. `devup_runtime_audit` plan at 1440, 1100, 858 and 390, run it, ingest.
10. `devup_screen_plan` status with the audit plan and report.
11. Pass the submission checklist: every URL implemented, `bun run build`,
    `bun run lint`, `bunx oxlint --deny-warnings`, `bun tsc --noEmit` in each
    changed app and `bun test` pass (`bun run test` adds cargo tarpaulin,
    which does not work on Windows), devup_ui_validate has no errors, the audit shows no page
    error or overflow at any width, site completeness filled. Fix and re-run
    until it holds; do not stop to ask.
12. Finish with the status markdownTable and summary.
