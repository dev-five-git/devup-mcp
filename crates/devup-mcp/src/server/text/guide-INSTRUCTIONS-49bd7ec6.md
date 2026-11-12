Build identity: use server.commit/buildId, not version; reconnect if stale. server.updateAvailable unknown never blocks.
0. GATES, never skipped: show the screen/URL table, ask with the question tool, wait; confirm only with userApproval (user's verbatim answer + every approved URL). No page write before confirm.
1. devup-mcp is the primary Figma-to-code source. Connection: bridge -> direct; devup_figma_auth status first; one Bridge needs no login.
2a. Another running project is no desktop lock; use split-screen.
2b. /devup: devup_project_plan intake; read Jira/spec/Figma and devup_project_context, ask all rounds up front, draft ERD, confirm, split. devup_screen_plan handles Figma. Load devup_skills and devup-implementation-plan; after confirm defer, continue, assume at the end.
3. Implement Figma with devup_figma_export tsx. Wire links/events (interactiveCandidates), devup_ui_validate, report wired/tested counts. deliverable.isFinal is export readiness. Audit every width, exact layout. Backend: devup_stack_diff zero drift.
4. Request only outputs you read. Before your second call read devup://guide/usage.
5. After confirm, extract COMPONENT_SET/COMPONENT first (componentTsx) into components/common|layout; map mainComponent id -> file in .devup/component-map.json. Assemble generated sections; audit via file mappings. Never hand-write TSX from Figma nodes.
6. Before completion apply developmentContract, behaviorPlan, keyframes motion; done only from status reportAllowed.
