# Evidence-backed application completion

`deliverable.isFinal` grades generated source only. It never means that an
application's interactions, animations or database bindings work. Use
`devup_runtime_audit` action `completion` for application completion.
Completion defaults to widths 360, 390, 858, 1100 and 1440; request these widths
in the browser plan, or pass the explicitly agreed required widths to both.

1. Export into the real project so structured `.devup/exports/` records retain
   behavior requirements. Follow every `behaviorPlan.pagination.nextAction` until
   complete. Keep a final signed plan in a private sidecar and pass its path in
   `behaviorPlanPaths` when a large capture required continuation. A partial page
   is never complete coverage.
2. Preserve generated TSX. For motion, use the supplied `motionControllerTsx`,
   `motionRuntime` and `motionBindings` outputs. Fill semantic selectors and real
   API values, then mount the shared controller once in the persistent layout.
   Generated controller code is executable; unresolved bindings remain explicit.
   Entrances are static CSS: use the supplied `revealTsx` (Devup UI keyframes on
   a view timeline) around each section's content group; no animation library.
3. Run actual build, lint, typecheck, unit test and functional E2E commands through the emitted
   `record-project-checks.mjs`, passing a JSON config with `projectRoot` and
   `checks: [{name:"build",command:["bun","run","build"]}, ...]`. The resulting
   `.devup/checks.json` records exits against a complete source/public asset
   digest; failed commands or source changes invalidate the implementation stage.
   Use separate `test` and `e2e` names; a unit test receipt cannot replace the
   functional E2E run. Run E2E with Figma reference visual tests for UI changes.
4. Create browser plans with explicit `projectRoot` so the server computes
   `plan.sourceDigest` from current files. Screen-plan `auditPlan` also binds this
   digest automatically. The runner independently hashes `plan.projectRoot`
   before and after browser execution, aborting stale or changed source runs.
   Execute the emitted runner and retain its plan, report
   and screenshots. Completion re-ingests each plan/report, rechecks hashes and
   requires every planned route/width, page health, overflow, clipped content,
   markup and rendered completeness. Motion checks additionally require the exact
   behavior candidate ID on the applicable planned viewport/screen/check index
   and observed intermediate behavior. Selected paint alone
   does not prove a sliding indicator or a complete step cycle. An entrance
   (`reveal`) is covered only by a passing check at a mobile (<=767px) and a
   desktop (>=1024px) width; an inconclusive check, where a scenario could not
   be observed, covers nothing (`motionEvidence` lists what is unverified).
   Include the exact Figma frame artifact for every drawn width. Completion
   reruns the ingested `visualComparisons` against cached native Figma PNGs;
   it never accepts a caller verdict or a path to an implementation baseline.
   Missing, failed or inconclusive comparisons block browser completion.
   Supply consumer-only renderer fields in `visualEnvironments`, keyed by
   comparison ID (tsxResource, themePath, assetDirectory, devupUiVersion and
   fontManifest). Observed viewport, capture context, renderer and actual PNG
   stay bound to the audited run. Retain actual/reference/diff and thresholds.
5. Operational data uses a typed `data-binding` check with actual OpenAPI/model
   anchors, exact exported binding IDs (`nodeId::role`), explicit aggregate and
   landing/list/detail selectors. The runner requires an isolated disposable DB,
   loopback API and nonce handshake before scenario mutations. It records ordered
   baseline, changed, empty and error phases. Rust independently rehashes the real
   schema files and SQLite snapshot bytes and re-executes only typed read-only
   aggregates; successful API responses alone cannot establish DB verification.
   Unsupported database adapters stay unverified. PostgreSQL asset URL migration
   SQL is a reviewable artifact, not proof that a live migration was applied.
6. Call action `completion` with `completion: {projectRoot, routes, widths,
   checkReceiptPath, audits:[{planPath,reportPath}], dataPlanPath,
   dataReceiptPath, behaviorPlanPaths, visualEnvironments}`. For inferred presentation values that are
   explicitly static, record an individual `staticBindings` explanation keyed by
   binding ID. A blanket success boolean cannot waive required evidence.

The tool reports `generated`, `implemented`, `browser-verified` and
`data-verified` stages with explicit blockers. It writes `.devup/completion.json`;
screen-plan status re-evaluates its inputs against current files rather than
trusting a saved `done` flag. No required data is reported as `not-required` scope,
never as a live DB connection. Command exits and browser measurements are
executor-observed; source digests, plan bindings, screenshots and DB snapshot
queries are independently rechecked. Fresh source or evidence changes require
verification again.

Internal clipping checks never waive an entire section merely because a child
animates. `allowedMarquees` names exact selectors and requires one visible,
horizontal-only clipped, infinitely translating nowrap track without controls.
Vertical clipping, unrelated siblings and invalid exemptions still fail.
