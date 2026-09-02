# Devup MCP Output Correctness Design

## Goal

Make generated DevupUI artifacts internally consistent, syntactically safe, and honest about whether the requested output is exact, approximated, or incomplete.

## Scope

This increment fixes four correctness contracts:

1. TSX and `devup.json` use the same canonical token name for every Figma variable and style.
2. Static JSX attribute values are escaped by one renderer before they are emitted.
3. Tool status and `strict` evaluate the requested projection, theme, and assets in addition to acquisition integrity.
4. Cached artifacts declare what was captured so an incompatible `artifactId` projection is rejected.

Section multi-root acquisition, MCP resource delivery, filesystem write confinement, visual diffing, and internal module decomposition remain separate follow-up increments.

## Token Identity

Variable token naming has one source of truth in the theme token module. A non-empty Figma `codeSyntax.WEB` wins; otherwise the final path segment of the variable name is normalized. Style tokens normalize the complete style name, matching the keys written to `devup.json`.

Codegen builds its ID-to-token maps through these shared functions. It must never independently camel-case a variable name. The emitted `usedTokens`, TSX props, theme JSON keys, and source maps therefore refer to the same token.

## JSX Attribute Safety

All static codegen props pass through a single attribute renderer. It escapes `&`, `"`, `<`, and `>` as JSX-compatible entities while preserving the existing deterministic quoted-attribute format. Both root/component props and nested styled text segments use this renderer.

Text child escaping remains separate because JSX text and JSX attributes have different grammars.

## Output Quality

Every completed tool result includes a `quality` object with four independent axes:

- `acquisition`: `complete`, `expected-projection`, `partial`, or `failed`
- `projection`: `exact`, `approximated`, `lossy`, `failed`, or `not-requested`
- `theme`: `complete`, `conflicted`, `unresolved`, or `not-requested`
- `assets`: `complete`, `partial`, `failed`, or `not-requested`

`Search` and `Explore` are intentionally compact projections. Missing descendants caused by that declared projection do not make the operation partial; missing roots, field failures, or truncated included fields still do.

Projection diagnostics are evaluated even when `includeDiagnostics` is false. Absolute positioning is `approximated`; mask and effect fallback are `lossy`. Theme conflicts and unresolved values affect the theme axis. Explicitly requested failed assets affect the assets axis.

The legacy top-level `status` remains for compatibility. It is `complete` only when all requested axes are acceptable, `partial` for approximated/lossy/conflicted/unresolved/partially failed output, and `failed` when acquisition or projection fails. `selection_required` and `needs_figma` remain workflow states rather than quality states.

For `strict=true`, every requested axis must be exact or complete. The error includes the quality object and acquisition report so clients can explain the rejection without exposing design content.

## Artifact Capabilities

Each cache entry records immutable, non-sensitive capture capabilities derived from the acquisition request:

- capture kind: `design`, `theme-only`, `search`, or `explore`
- collection scope
- resource scope

The metadata is returned with the artifact summary. Reuse through `artifactId` validates the requested outputs before projection:

- search and explore artifacts cannot produce TSX, raw snapshots, source maps, asset manifests, or themes;
- theme-only artifacts can produce only `devupJson`;
- file-scoped `devupJson` requires file resource coverage;
- node/page theme requests cannot exceed the captured collection/resource scope;
- design-only outputs require a design artifact.

Invalid reuse returns `DEVUP_FIGMA_HANDOFF_INVALID` and performs no projection or filesystem write.

## Compatibility and Privacy

Existing output fields remain present. The new quality and capability metadata contains counts and enum values only, never Figma text, variable values, tokens, URLs, OAuth data, or raw payloads.

All Figma access remains read-only. This increment does not broaden OAuth scope or add network calls.

## Verification

- Unit tests prove canonical token naming and JSX escaping.
- MCP integration tests prove output quality, operation-relative Explore completion, strict projection rejection, and incompatible artifact rejection.
- Existing 268 upstream JSON golden fixtures remain byte-identical unless the shared token or escaping correction intentionally changes their output; any reviewed changes must be explicit.
- Workspace fmt, clippy, all-feature tests, snapshot checks, and release build must pass.
