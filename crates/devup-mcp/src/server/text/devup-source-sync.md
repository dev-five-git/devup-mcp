Reconcile a new design TSX with an existing application through a read-only
three-way plan, then atomically apply the reviewed plan. The original export
record stays intact; caller-supplied TSX is an unverified design proposal.

Pass projectRoot, baselineRecord (a .devup/exports JSON path), incomingTsx and
explicit .tsx files, including extracted components. action=plan returns
proposedSources, semantic/structural mappings, confidence, conflicts,
expectedHashes and planHash. Review these outputs. action=apply requires both
expectedHashes and expectedPlanHash from that exact dry run. A stale file,
changed proposal or concurrent sidecar update refuses the write.

Design style/layout attributes merge only when the corresponding application
attribute still equals the baseline (format and quote spelling may differ).
Imports, API reads, event handlers, ARIA and browser-state wiring remain in
place. A static literal copy update is safe only when the current literal is
unchanged and the AST mapping is unique. Dynamic API text, edited literals,
spreads affecting precedence, ambiguous duplicates and unsupported structural
changes produce conflicts. No component body is replaced to resolve a conflict.

Successful apply writes sources and .devup/source-mappings.json in one guarded
transaction. The sidecar binds file hashes and actual semantic/structural match
evidence; source order alone never establishes a Figma or runtime DOM identity.
Repeated calls use a separate sourceSyncState design baseline and retain the
original capture provenance. Run UI validation, build and browser audits after
applying; source synchronization is not runtime verification.
