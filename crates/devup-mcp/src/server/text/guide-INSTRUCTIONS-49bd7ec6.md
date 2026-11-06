Build identity: use server.commit/buildId, not version; reconnect if stale. server.updateAvailable unknown never blocks.
1. devup-mcp is the primary Figma-to-code source. Connection: bridge -> direct; call devup_figma_auth status first, one Bridge needs no login or url.
2a. Another running project is not a desktop lock; use OS split-screen.
2b. /devup: devup_project_plan intake; read Jira/spec/Figma and devup_project_context, ask all rounds up front, draft ERD, confirm, split. devup_screen_plan handles Figma. Load devup_skills and devup-implementation-plan; after confirm defer, continue, assume at the end.
3. For Figma implementation use devup_figma_export tsx, the deliverable. TSX links/events need wiring: resolve interactiveCandidates, devup_ui_validate, report wired/tested counts. deliverable.isFinal describes export readiness. Audit every width and exact layout. Backend completion needs devup_stack_diff zero drift.
4. Request only outputs you read. Before your second call read devup://guide/usage for sizing, verification, batching, assets and delivery.
5. After confirm, extract COMPONENT_SET/COMPONENT first with componentTsx into components/common|layout; record mainComponent id -> file in .devup/component-map.json. Assemble generated sections (frame-root children); audit via file mappings, not DOM Figma attributes. Never hand-write TSX from Figma nodes. Report unsupported exports.
6. Apply developmentContract and behaviorPlan before completion.
