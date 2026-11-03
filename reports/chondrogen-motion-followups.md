# Motion implementation and resumable behavior inventory

This implements follow-ups 4, 5, 8 and 13. Generated page TSX keeps its captured
layout. Inferred motion is still distinguished from captured Figma prototypes;
the export now provides working controllers as concrete application source.

## Delivered files

When a page contains a supported motion candidate, its export includes:

- `motionControllerTsx` / resource `MotionController.tsx`: one small Client
  Component that mounts and cleans up the page's shared DOM controllers.
- `motionRuntime` / resource `motion-runtime.ts`: seamless left marquee,
  once-on-entry integer/decimal CountUp, progress-complete/advance/wrap with manual
  reset, persistent route-tab indicator, and shared section entrance observation.
- `motionBindings` / resource `motion-bindings.json`: candidate IDs, captured
  values/precision/units/prefixes and required semantic selector bindings.

All three also accept corresponding `outputPaths` keys. Keep the two source files
adjacent under their delivered names. Mount `MotionController` once in the shared
server layout; server-rendered content and data fetching stay on the server.
The effect is limited to attaching/cleaning DOM observers and animation frames.

No theme colors, Figma IDs, inspection attributes or metadata comments are added
to application code. An existing captured indicator/progress track supplies its
appearance. Captured partial progress fills can bind `progressTrack` to their
full track. Bind real URL/ARIA selection and validated API values; unresolved
targets are explicit, and an implemented reusable controller does not claim the
particular application has already wired or browser-verified it.

Continuous marquee/step playback adds no pause buttons. Reduced-motion behavior
is the default; an explicit continuous-playback requirement can set
`respectReducedMotion: false`. CountUp reserves the final responsive text width
and exposes the final accessible value. Marquee repeats are inert and hidden
from assistive technology. Reveals preserve visible server/no-JS content, share
one section observer, support inserted route content, and cancel on focus.

## Actual motion checks

`devup_runtime_audit` accepts `kind: "motion"` with a `motion` configuration:
`kind` (`marquee`, `count-up`, `steps`, `tabs`, `reveal`), exact `candidateId`,
semantic `target`, kind-specific selectors, duration and optional resize width.
The result's `measured.candidateId` binds the observation to the behavior plan.

Checks measure intermediate geometry and final state, tab DOM continuity in both
directions, actual marquee movement and seam geometry, partial/final counts and
reserved width, step fill/advance/manual reset, and reveal translation/opacity.
A pre-navigation count probe retains bounded initial-entry samples, so an audit
can verify a one-shot above-fold count that finished during normal readiness
checks. A static final number cannot satisfy the motion check. Screenshot-frozen
animations are restored during motion checks and frozen again afterwards.

## Statistics and continuation

The numeric range 1900–2100 no longer discards a statistic such as `2000건`.
Calendar names/formats, IDs, row/pagination indices, malformed grouping and
leading-zero identifiers remain excluded. Negative decimals, decimal precision,
units and common currency prefixes retain their meaning.

Each behavior page bounds node inference and output rows. Its signed cursor
retains DFS work, visited IDs, emitted candidates/bindings and pending work.
Reveal and palette candidates are budgeted incrementally as well; they cannot
silently fall off a post-traversal truncation. The artifact graph/fingerprint is
read for context, separately from the per-page inference limit.

Follow `behaviorPlan.pagination.nextAction` using the existing
`devup_figma_export` operation with the original `artifactId` and `behaviorCursor`.
Continuation supports normal resource delivery and optional generated motion /
`behaviorPlan` output paths. Save each full page and merge candidates by `id`,
bindings by `nodeId::role`, and page-shell contracts by their source identities.

The final plan provides the exact aggregate candidate inventory and binding keys
with an authenticated artifact/snapshot/scope-bound coverage receipt. Completion
uses `verified_coverage_inventory`; changing the inventory, scope or source
invalidates the receipt. Cursors/receipts are server-session bound and artifacts
retain their normal TTL. A server restart or expired artifact requires a fresh
export; it is never reported as successful continuation.

## Regression evidence

- `behavior_contracts/followup_tests.rs`: statistics, resource source bytes,
  deterministic continuation, no duplicate/omitted candidates/bindings, wrong
  artifact/snapshot rejection, full node-limit continuation, authenticated final
  inventory, and bounded section-candidate cursor size.
- `runtime_audit/motion_validation.rs`: candidate identity, semantic selector and
  timing/value validation.
- `harness/runtime-audit/test/motion.test.mjs`: production template compilation
  and real Chromium observations for all five controller families, integer 2000,
  negative/positive decimals, both tab directions, manual step navigation, resize,
  newly inserted sections, no-JS visibility, teardown and static-number failure.
