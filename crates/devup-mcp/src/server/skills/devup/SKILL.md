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
   tokens the code must use) and `scope: "routes"` (the pages that exist).
4. **Screen plan.** `devup_screen_plan` with `action: "draft"`, `projectRoot`,
   the link and the notes. It returns the screens, the excluded areas, the
   defaults it will fill in, and every open question - routes, sections and
   structure, and the site questions (favicon, share image, site name and URL,
   analytics, motion).
5. **Ask everything now, with your question tool.** The flat `questions` list
   in one call if your tool has no per-call cap; otherwise `questionRounds`
   back to back. Never in prose, never one decision at a time, never later.
6. **Confirm.** `devup_screen_plan` with `action: "confirm"`, the `planId` and
   every answer that applies.
7. **Implement URL by URL without asking anything.** `devup_figma_export` with
   `projectRoot` and each screen's `frameIds`; `pageScaffold` takes its route
   from the plan. Add the plan's `defaults` as you go: `_hover`,
   `_focusVisible`, `_active`, `_disabled` and `cursor="pointer"` on every
   interactive element from existing tokens, one shared transition,
   `_motionReduce`, root and per-page `metadata` with openGraph, `lang`, alt
   text, image dimensions, a not-found page. Run `devup_ui_validate` with
   `projectRoot` on what you wrote. Anything new: take the recommended or most
   conservative option and record it with `action: "assume"`.
8. **Runtime audit.** `devup_runtime_audit` `plan` with every confirmed URL at
   1440, 1100, 858 and 390, run the executor in the project, then `ingest`.
9. **Status.** `devup_screen_plan` with `action: "status"`, `auditPlanPath`
   and `auditReportPath`.
10. **Final table.** End with the status `markdownTable` and `summary`: URL |
    Figma screen | file | status | widths verified | screenshots, then site
    completeness, design fixes requested, excluded frames, the defaults filled
    in and every assumption.
