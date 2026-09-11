# R14 asset evidence

`asset-observation.json` is the unmodified `details.originalValue` of the first
ABSOLUTE diagnostic for `3997:46668` in WQUW-118's saved response:
`C:/Users/owjs3/orca/workspaces/girok-space/wquw-118-fr007-question-trial-send/docs/verify-r13/evidence/016-devup_figma_export.response.json`.
Server commit: `009491afd089`. Source repository was read only.

`asset-snapshot.json` is a reduced regression fixture reconstructed from that
diagnostic's node, parent and first child. Diagnostic metadata is removed;
null fields are omitted (the source reports `constraints` in `missingFields`).
The parent and group child lists connect only these three nodes. It is not a
new Figma acquisition or a complete raw snapshot. Geometry, sizing, first-child
SCALE/MAX constraints and paints are preserved. Tests separately cover explicit
null and missing constraints. Unrelated screen siblings and tokens are absent.

Type 1 uses the existing full `fixtures/r8/modal-snapshot.json`, independently
confirmed against WQUW-120's `docs/verify-r13/evidence/06-poll.json` and the
WQUW-119 R12 report. No TSX golden regeneration is intended.
