# Design fingerprint sidecars and capture comparison

Base: `c2b4e31944e69035d233f217ce0f1d5fb5e02fb4`.

## Contract

Request `designFingerprints` in `outputs` and set
`outputPaths.designFingerprints` to an allowlisted file path to persist a JSON
sidecar alongside generated code. The sidecar uses a
`kind: "devup-design-fingerprints"` discriminator, an explicit
`fingerprintVersion: "v1:sha256:"`, the node-ID-keyed `designFingerprints` map,
and `overwritePolicy: "replace-existing"`.

The chosen overwrite policy follows existing ordinary export files: a requested
write replaces an existing file using the guarded, staged-then-committed output
transaction. The sidecar states this policy in the response. A combined scaffold
request still honors its stricter preview/no-overwrite transaction settings.
There is no second writer, filesystem read input, or change to hashing.

To compare a later capture, pass the saved JSON text as
`previousDesignFingerprints` and request `designChanges` in `outputs`. The
comparison reports sorted node IDs in `unchanged`, `changed`, `added`, and
`removed`. Neither new output is implicitly returned or written.

Malformed, truncated, unrelated, or invalid-hash sidecars are refused as
`DEVUP_INVALID_INPUT` with `details.type: "designFingerprintMalformed"`.
Version mismatches are refused with
`details.type: "designFingerprintVersionMismatch"`, `previousVersion`, and
`currentVersion`; both versions also appear in the error message. The version
envelope allows empty maps to be validated. Foreign versions are never compared.

## Deliberate limits

- Changed means a field the converter reads changed. It does not establish a
  rendered-screen change and does not locate a change inside a node.
- Unchanged means captured converter inputs for that node are identical. It does
  not establish that generated code on disk still matches: a human may have
  edited that code.
- Added and removed describe absence in the two supplied capture maps. They do
  not establish deletion from the entire Figma document; callers should compare
  the same capture scope.
- These are source fingerprints, not generated-code, external-variable, asset
  byte, or pixel hashes. No character or byte offsets are exposed.

## Verification

All build/test commands use this worktree's own target directory with
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, and
`CARGO_INCREMENTAL=0`; builds use `-j 2` (or `CARGO_BUILD_JOBS=2` for the exact Insta and stdio commands).

### Baseline and RED evidence

Baseline `cargo test --workspace -j 2 --no-fail-fast`: **1011 passed, 0 failed, 2 ignored** (98 result summaries), exit 0. The unchanged Windows build emitted localized MSVC linker stdout warnings.

RED `cargo test -p devup-mcp --lib -j 2 w10_`: **1 passed, 12 failed, 0 ignored**, exit 1. This run occurred before implementation, with empty comparison/parser scaffolding and the existing export path; the unrequested-output guard already passed. Failed tests:

- `server::design_drift::tests::w10_added_node_is_not_unchanged`
- `server::design_drift::tests::w10_comparison_explains_limits_without_rendered_equivalence`
- `server::design_drift::tests::w10_identical_capture_is_all_unchanged`
- `server::design_drift::tests::w10_malformed_truncated_and_unrelated_sidecars_are_typed_errors`
- `server::design_drift::tests::w10_one_converter_read_field_changes_exactly_one_node`
- `server::design_drift::tests::w10_removed_node_is_not_unchanged`
- `server::design_drift::tests::w10_sidecar_round_trips`
- `server::design_drift::tests::w10_version_mismatch_names_both_versions`
- `server::projection::w1_regressions::w10_comparison_requires_previous_sidecar`
- `server::projection::w1_regressions::w10_sidecar_and_code_cannot_silently_share_a_path`
- `server::projection::w1_regressions::w10_sidecar_outside_allowed_root_refuses_entire_transaction`
- `server::projection::w1_regressions::w10_sidecar_write_uses_guarded_transaction_and_reports_replacement`

### Final checks

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings`: exit 0, **0 warnings**.
- `cargo test --workspace -j 2 --no-fail-fast`: exit 0, **1026 passed, 0 failed, 2 ignored**, across 98 result summaries (**15 added passing tests**).

- `cargo insta test --workspace --all-features --check`: exit 0, **1026 passed, 0 failed, 2 ignored**; no snapshots to review.
- `cargo test --locked -p devup-mcp --test stdio_smoke`: exit 0, **2 passed, 0 failed, 0 ignored**.

The first GREEN attempt reached 12 passed and one Windows-only fixture cleanup
failure: the test still held its allowlisted directory handle while deleting the
fixture. Dropping that handle before cleanup fixed the test; the path-rejection
and unchanged-file assertions already passed. The next focused run passed all
15 tests, including response-to-sidecar round trip, sourceMap agreement, resource
retry preservation, output filtering, and collision refusal. No production path
checks or rollback behavior were weakened.

### Usage

Persist a capture alongside generated code (replace `artifactId` and use paths
under the server's configured write roots):

```json
{
  "artifactId": "<retained artifact ID>",
  "outputs": ["tsx", "designFingerprints"],
  "outputPaths": {
    "tsx": "src/Screen.tsx",
    "designFingerprints": "src/Screen.design-fingerprints.json"
  }
}
```

For the later export, request `outputs: ["designChanges"]` and set
`previousDesignFingerprints` to the **entire saved sidecar JSON as a string**.
Use a refreshed URL acquisition or a newer retained artifact for the current
capture. Reusing the same retained artifact compares that same capture, not live
Figma. No sidecar file is read by the server; the caller supplies its contents.

`outputPaths.designFingerprints` is supported only when the sidecar output was
requested. Successful writes appear in `outputPaths.designFingerprints` and
`outputPathResults.committed.designFingerprints`; supported keys and unsupported
path diagnostics retain their existing contract. Differing code and sidecar
bytes cannot silently share a target path. Resource delivery moves explicitly
requested bodies into the existing resource store and preserves the previous
sidecar in reprojection arguments.

## Delivery and cleanup

The implementation and this report are committed together on this worktree's
branch; the coordinator completion message names the exact commit. No push is
performed. `cargo clean` runs after that commit to release this worktree's build
artifacts, with its observed result reported in the coordinator message.

The settled `provenance/fingerprint.rs` computation and all prohibited modules
remain unchanged. The MCP router adds only validation/forwarding, while the new
`server/design_drift.rs` contains the pure comparison, strict sidecar parser, and
requested-output assembly. The existing output transaction owns every file
write and retains its reverse-order rollback and path guarding.
