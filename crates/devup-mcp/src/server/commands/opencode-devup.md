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
4. `devup_screen_plan` draft with projectRoot, the link and the notes.
5. Ask every question now with the `question` tool: the flat `questions` list
   in one call. Never in prose, never during implementation.
6. `devup_screen_plan` confirm with every answer that applies.
7. Implement URL by URL without asking anything: `devup_figma_export` with
   projectRoot and frameIds, plus the plan's defaults - hover, focus-visible,
   active and disabled states from existing tokens, one shared transition,
   reduced motion, metadata with openGraph, lang, alt text, image sizes, a
   not-found page. Validate with `devup_ui_validate` and projectRoot. Record
   anything new with `devup_screen_plan` assume.
8. `devup_runtime_audit` plan at 1440, 1100, 858 and 390, run it, ingest.
9. `devup_screen_plan` status with the audit plan and report.
10. Finish with the status markdownTable and summary.
