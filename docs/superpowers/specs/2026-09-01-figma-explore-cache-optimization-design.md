# Figma Explore Cache Optimization Design

## Goal

Reduce repeated `devup_figma_explore` latency without weakening target identity, source-policy isolation, bounded memory, or freshness controls. The first official Figma read remains authoritative; compatible follow-up reads reuse its in-memory projection.

## Scope

- Add `refresh` to `devup_figma_explore` and bypass every cache and in-flight reuse path when true.
- Reuse an exact cached projection for the same request.
- Reuse a cached projection for another node only when that node exists in the cached snapshot.
- Reuse a larger explore projection for a smaller requested projection when every non-limit request dimension matches.
- Let concurrent direct acquisitions wait for a compatible in-flight projection, then reuse it only if it contains the requested node; otherwise acquire independently.
- Report cache reuse kind, age, remaining TTL, avoided Figma calls, origin collection statistics, and current-request collection statistics.
- Mark Git builds made from a modified worktree with `-dirty`.
- Measure the completed behavior against WQUW-151.

Disk persistence is excluded because official explore payloads may have a null Figma version. Persisting such payloads across MCP restarts could return unverifiable stale designs.

## Cache Compatibility

An explore artifact can serve a request only when file key, branch key, collection scope, resource scope, context flags, metadata flags, asset/reference requirements, source policy, and text-preview limit match. The cached `projection_limit` must be greater than or equal to the requested limit. Node IDs may differ only when the requested node exists in the cached snapshot.

Selection prefers an exact key, then the smallest compatible projection, then the most recently accessed artifact. `refresh=true` skips exact lookup, related/superset lookup, and compatible in-flight waiting.

## Concurrent Direct Acquisition

The artifact store records the request key beside each exact in-flight acquisition. A compatible follower waits for the owner. When the owner completes, the follower reuses the artifact only if the requested node is present. If it is absent or the owner fails, the follower starts its own acquisition. This serializes potentially related direct reads safely; it never invents coverage.

Host handoffs are not shared while incomplete because exposing the same one-shot call ID to multiple clients would violate replay protection. Once a host handoff completes, all compatible cache reuse rules apply.

## Public Diagnostics

`cache` gains:

- `reuseKind`: `miss`, `exact`, `related-node`, `superset`, or `related-node-superset`.
- `ageSeconds` and `remainingTtlSeconds`.
- `avoidedFigmaToolCalls`.
- `originCollection`: statistics from the acquisition that created the artifact.

Top-level `collection` describes only the current request. It is zeroed for a cache hit and retains acquisition statistics for a miss. Existing cache identity, hash, capability, size, and timestamp fields remain.

## Freshness and Safety

`refresh=true` always creates a new artifact and replaces the exact-key entry after successful acquisition. A failed refresh does not delete the previous valid artifact. Cache entries remain memory-only, bounded by the existing entry and byte limits, and expire after ten minutes.

## Build Identity

When `DEVUP_MCP_BUILD_ID` is absent, the build script uses the twelve-character Git commit and appends `-dirty` if tracked or untracked non-ignored files differ from HEAD. Clean builds retain the plain commit ID. Non-Git builds remain `source-unknown`.

## Verification

- TDD integration tests cover refresh, exact/related/superset selection, incompatible options, current-vs-origin statistics, and completed host reuse.
- Artifact-store tests cover compatible in-flight direct acquisition and unrelated fall-through.
- CLI tests cover clean/dirty build ID formatting through a testable helper.
- Full workspace tests, Clippy with warnings denied, formatting, Node script tests, and snapshot checks must pass.
- WQUW-151 measurement reports first official Figma duration, repeated exact and related-node local durations, Figma call counts, cache reuse kinds, and calculated time saved.
