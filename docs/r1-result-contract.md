# R1: output truth contract

Hidden nodes and their descendants do not contribute asset references to generated TSX. Hidden assets remain discoverable in assetSummary.unavailable with reason hidden-node; they are excluded from the deliverable manifest. A hidden asset warning is not evidence that a visible, required asset was collected.

Projection deviations are result data. Each frame and the top-level result exposes projectionIssues by default, independent of includeDiagnostics. Entries carry code, nodeId, property, message and details with originalValue and appliedValue. Fidelity impact follows the diagnostic code unless explicitly supplied as fidelityImpact. Applied values identify generated CSS props or source excerpts, rather than claiming that the source value was preserved exactly. Missing source evidence is null and explicitly described. Unclassified uncovered layout is reported as lossy; the server does not invent an intentional-exclusion explanation. Acquisition continues to describe collection independently.

quality.assets grades binary collection, not whether a manifest was requested. assetSummary.description explains its collection status; manifestIncluded independently describes manifest delivery. not-collected means discovered assets have no collected bytes, not-requested on an unavailable item means its byte capture was not requested, none means complete discovery found no assets, unknown means discovery cannot establish absence, partial means some bytes or discovery are incomplete, and collected means all discovered deliverable assets have bytes. Hidden exclusions are reported separately and do not require bytes.

The recommended batch is 1–3 frames. Budget responses use recommendedBatchSize at most 3 and expose maxFrameCount=6 and maxFrameOutputUnits=12 as hard limits. recommendedFrameIds and remainingFrameIds split at the recommendation, not at the maximum.

Validation uses the checkout-local Cargo target, never installed MCP tools. The saved production responses are evidence of old behavior, not executable source captures; regression fixtures model the confirmed source paths.

Responsive issues label per-breakpoint evidence as `evidenceScope: breakpoint-source-projection`; this preserves source limitations without claiming a pixel-level validation of the merged result. Actual capture results override discovery predictions by assetId. A path supplied by another non-excluded asset is still valid even when a hidden asset shares that name.
