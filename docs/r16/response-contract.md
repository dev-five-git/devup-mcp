# R16 response explanation contract

R16 changes response metadata only. Existing resolution strings, classification thresholds, R13 property provenance, R14 componentSummary/widthPreservation and R15 derived evidence remain authoritative.

## Mapping and bounded verification

`sourceMap.resolutionSemantics` documents the mapping-method axis and its values. Section frame maps use `dictionary: "/resolutionSemantics"` to reference one shared dictionary on the aggregate response (the pointer is relative to that aggregate object, not the frame); it contains the labels used by the selected outputs and remains inline during resource delivery. This avoids repeating the dictionary per frame and preserves the existing inline-size contract. Standalone source maps carry the dictionary directly. Join entries to diagnostic components by nodeId and source property (width → width, height → height, x → horizontal, y → vertical), within the same screen and output. `raw-fallback` is the generic source-value/generation-policy mapping route; it is not a low-confidence grade or a claim that raw values were copied verbatim. It remains correct for `w="100%"` even when the ABSOLUTE width component separately verifies the equal FIXED containing-block relation. Neither label measures browser pixels or responsive equivalence.

Some legacy mapping labels include a narrow value check (`verified-explicit-dimension`), but they are never substitutes for the ABSOLUTE component proof or whole-output verdict. The response dictionary describes each label's scope.

## Blocked dimension proofs

`components.width.blockedBy` is mirrored in `appliedValue.widthPreservation`. Dimension components return null for a successful bounded proof and a stable reason string when blocked. A reason describes the first blocking check for the applicable proof, not all failures and not evidence that other checks passed. It describes this invocation only: it does not claim every negative branch was executed against the live design.

Required distinct reasons: `parent-width-unknown`, `parent-width-unequal`, `read-error`, `non-fixed-sizing`, `conflicting-dimension-props`. Further reasons distinguish padding/borders, an unproven containing block, unavailable source dimensions, unproven generated dimensions, and export-boundary/rounding failures. Asset dimensions use the selected render-boundary proof rather than an irrelevant parent-percentage reason. Successful boundary/rounding checks still do not promote an unresolved position axis.

Additional blocker codes retain the same acceptance predicates:

| Code | Blocking condition |
| --- | --- |
| `parent-padding-or-border` | Parent padding/borders prevent the percentage proof. |
| `containing-block-unproven` | Required emitted-parent/auto-layout relation is unproven. |
| `source-dimension-unknown` | No finite source dimension. |
| `generated-dimension-unproven` | Emitted size has no applicable successful fixed/intrinsic/percentage proof. |
| `intrinsic-layout-unproven` | HUG omission lacks a matching auto-layout proof. |
| `export-boundary-unproven` | Selected export boundary or its parent relation is unproven. |
| `rounded-dimension-mismatch` | Generated pixels fail the selected-boundary serializer check. |
| `parent-height-unknown` / `parent-height-unequal` | Corresponding existing percentage-height proof is blocked. |

## Aggregate scope

`verdictScope` accompanies export results and individual frames, including resource delivery and `includeDiagnostics=false`. It echoes status/projection and lists `statusCauses` by quality domain, plus requested-output failures. `projectionCauses` indexes final projection issues by screenId, nodeId, output, code and property. ABSOLUTE issues expand only unresolved components, with their current state, reason and resolution condition. Verified/preserved dimensions are excluded from these causes even when the same node's horizontal/vertical axes remain approximated.

This is a summary derived from existing final verdicts and diagnostics. It does not reclassify them, erase lower-severity issues, invent missing node IDs, or certify a rendered screen.

Quality-domain evidence uses `evidenceScope`: projection causes live on the same object (`self`), while acquisition/theme/asset reports live on the aggregate `response`. Frames retain their own filtered `failures`; each requested-output cause's `failureIndex` points into that object's array. Successful sibling frames do not inherit another frame's failures.

This contract covers final export result objects (including partial results), not selection/in-progress responses or strict-mode rejection errors. Existing strict rejection details and diagnostics remain unchanged.
