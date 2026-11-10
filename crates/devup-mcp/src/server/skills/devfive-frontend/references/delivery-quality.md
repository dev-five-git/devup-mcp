# Behavior and data contracts

Apply these rules with every export, including code assembled from multiple
frames. `developmentContract` and `behaviorPlan` are implementation obligations,
not proof that controls, API integration or animations already work. The running
binary ships this document in its usage resource and skill installation.

## Preserve the design in production code

- Use generated TSX as the layout source. Preserve captured instance overrides,
  text and all widths, using responsive arrays and one shared tree whenever the
  content is the same. A changed wrapper alone is not a different component.
  Keep truly different mobile cards and desktop tables separate.
- Application TSX is complete code: no `@devup-*` metadata comments and no
  `data-figma-node-id(s)` attributes, including debug builds. Node/element maps,
  overrides and interaction evidence live in JSON sidecars. Resolve every
  override through a prop/variant or record a reason; never discard its evidence.
- JSX uses actual `$token` props, including responsive lengths; no `var(--token)`
  in JSX. External CSS objects use CSS variables where the library requires them.
  Do not invent tokens from matching numbers. Collapse equal `pl/pr` to `px`,
  `pt/pb` to `py`, and matching margin pairs, only when precedence is unchanged.
  Verify computed styles at each breakpoint after changing token syntax.
- Korean text keeps word boundaries: `wordBreak="keep-all"` plus
  `textWrap="pretty"` (rebalances lines so keep-all leaves no large trailing
  gap) and `overflowWrap="break-word"` for an overlong token. Never use
  `wordBreak="normal"` on Korean copy. Explicit Figma line breaks are still
  preserved, `pre`/`nowrap` text gets no `textWrap`, and explicit theme/local
  wrapping props win. A visual diff caused only by Figma breaking inside a
  Korean word is an intentional, documented policy difference: record it, and
  never overwrite the native Figma reference to make the comparison pass.

## Shared server shells and usable controls

- Plan `layout.tsx` ownership before assembling pages. Persistent headers,
  footers and route-tab bars belong in shared server layouts; data pages remain
  Server Components. Put only pathname-dependent selection, focus handling,
  animation or local state into a small Client Component. Do not add memoization
  wrappers for work already handled by React Compiler.
- The page shell grows to at least the viewport (`minHeight: 100dvh`, column flex,
  growing main content or auto top margin on footer). Preserve designed spacing;
  do not pin/fix the footer over content. Check short, empty and long pages.
- Actual links/buttons/selects must own hover, focus-visible, active, disabled,
  pending and captured state styles. Capture the whole COMPONENT_SET before
  projecting states; a static icon or screenshot is not a working select.
  Verify select open/choose/close, arrows, Enter/Escape, ARIA and focus restoration.
- A colour variant must use its own captured palette for background, text,
  border, icon, hover, press, focus and disabled states. Do not inherit primary
  blue into a secondary variant or infer absent tokens by suffix number.
- Derive content, selected background/text/icon and `aria-current` or
  `aria-selected` from the same normalized URL/state. Never hardcode a frame's
  selected appearance into a desktop/mobile copy. Keep filters/sort when changing
  view, reset pagination deliberately, and test both directions, direct URLs,
  reload, back/forward, keyboard activation and resize.

## Motion inferred from visual meaning

Inspect visual cues and sibling designs even when no prototype animation exists.
Treat inferred motion and timing as implementation choices, not captured facts.
Use captured variant text/images for each state. Respect explicit user playback
preferences; do not introduce unsolicited pause controls or hover suspension.

- Overflowing repeated single-line display text is a marquee candidate: two
  identical track copies, constant linear translation and a seamless loop. Hide
  duplicate content from assistive technology; measure after font load and resize.
- Emphasized statistics are count-up candidates across all pages, not just the
  landing. Support integers, decimals and dynamic API values. Exclude dates,
  identifiers, patent numbers, labels, table row indexes and pagination. Preserve
  units, precision, the final accessible value and reserved layout width. Start
  once on viewport entry; verify final values after resizing and navigation.
- Sibling active-step designs plus progress lines suggest timed cycling:
  complete progress, then highlight/description/image advance, wrap to the first
  step, and reset deliberately on manual selection. Derive length from data,
  handle empty/one-item arrays and clean timers/listeners on unmount.
- Rounded route tabs suggest a translating shared highlight. Keep the bar mounted
  in a common layout; measure actual link widths/positions after route and size
  changes. Test animation calls, intermediate position and persistent DOM, not
  only the final selected colour. Retain native link/keyboard semantics.
- Use restrained below-to-above entrance motion for meaningful sections, as
  Devup UI static keyframes on a view timeline. The one shared controller only
  marks blocks for browsers without view timelines and for groups on a clock,
  including sections added after navigation. Do not hide SSR content before
  JavaScript runs. Support reduced motion, no-JavaScript, focus, one-time
  playback and observer cleanup.
- Add transitions for captured colour, border and state changes without animating
  every layout property. Reduced-motion behavior is the accessible default unless
  an explicit requirement says otherwise; document intentional exceptions.

## Entrance and transition motion is a default item

Do this without being asked, on every page built or changed; the installed
`devfive-frontend/references/animation.md` has the full rules. A design the
user stated (no animation, a given timing, a replaying effect) always wins.

- Survey sections and repeated content, then propose motion by role and
  implement it: one restrained entrance per below-fold content group, a short
  stagger for card groups only where it helps, transitions for state changes.
  Do not animate every heading or control, the first viewport, body copy,
  forms or navigation.
- Implement it with Devup UI `keyframes()` and CSS animation props, extracted
  to static CSS: no animation library, no bundle, no hydration flash, only
  `opacity` and `transform`. A scroll reveal is
  `animationName={rise} animationFillMode="both" animationTimeline="view()"
  animationRange="entry 0% entry min(100%, 240px)"` on an inner wrapper, with
  `_motionReduce={{ animation: 'none' }}` and
  `_focusWithin={{ animation: 'none' }}`. The hidden start lives only in the
  keyframes' first frame, never on the element. A group on a clock (staggered
  cards, a progress line) is `data-reveal="timed"` with
  `animationDelay: calc(var(--reveal-index) * 60ms)` on its items; the shared
  controller marks it once. Use framer-motion only when the project already
  depends on it and the user asks, or for exit and gesture motion.
- A scroll reveal starts when its content is actually visible and plays once;
  re-entry, scrolling back and back/forward never replay or re-hide it. A
  section with a background keeps the painted element fixed and moves an inner
  wrapper. A progress line or graph shares one trigger and timeline with the
  items it connects.
- Server markup stays readable: no content that starts at `opacity: 0` or
  translated away waiting for JavaScript, and no visible content hidden on
  hydration to replay an entrance. Reload, client navigation and a restored
  scroll position start from visible content. No-JavaScript, delayed scripts,
  `prefers-reduced-motion` and keyboard focus all show content in place.
- Verify the animation, not the final frame: start, middle and end states and
  the real trigger position at one mobile and one desktop width, plus scroll
  re-entry, fast scroll, back/forward (new document, bfcache and client
  navigation), delayed and failed images, focus, reduced motion, no
  JavaScript, and for groups the stagger step, span and total time and the
  progress line against its items. `devup_runtime_audit` measures all of it
  (`kind: "reveal"`) and reports `motionEvidence`. Report each as verified or
  explicitly unverified; what could not be checked is never a pass.

## One source for business data

Inventory every statistic, product summary and list/detail field before declaring
integration complete. A landing patent count uses the same API aggregate as the
patent page; pipeline totals and categories derive from actual records. Labels
such as total/registered are immutable text, not numeric replacement targets.
Preserve captured/API list ordering and pagination-aware ascending or descending
display ordinals separately from record IDs. Research years and editorial landing
copy need validated stored fields, not
duplicated TSX literals. Reuse product names, descriptions and stages on landing
and detail pages. Do not infer an API endpoint from a design label.

Use parallel server reads and minimal client boundaries. Never present fixture
values or local success state after API failure. Test DB -> API -> UI using a copy
DB: change counts/categories/text, empty the records, and simulate server failure.
For forms verify actual stored fields and rejected submissions, preserving real
records. Distinguish persistence from email delivery or deployment. Migrations
must preserve edits, be idempotent and run when development seeds are disabled.

## Every committed asset has a purpose

Use one project registry (for example `docs/public-assets.json`) across SVG,
WebP/PNG/JPEG, PDFs, social previews and other public files. Public filenames
describe content and role, never content hashes, Figma IDs or `web-ready` dumps.
Keep source hashes/IDs in sidecars. Query the registry before acquiring/writing;
repeat exports must reuse paths and never overwrite different existing content.

SVGs share one original across sizes; set responsive `boxSize/w/h` on one element.
Grayscale/muted logos share the colour original with `grayscale(1)` and opacity.
Flat glyph colours come from masks/tokens, not per-state SVG files. Normalize
internal IDs and scale for identity comparison without corrupting mask/filter
paint, alpha, viewBox padding or intentional optical differences. The same small
numeric difference is not sufficient proof of equality.

For raster media compare actual content, crop and resolution. Reuse an appropriate
original without arbitrary re-encoding/upscaling; retain a distinct image only
when the crop/content requires it. Before removing/renaming, collect code, CSS,
API seed, persisted DB and dynamic references; ignored export records are not
production usage. Update references together and provide a safe URL migration.
Do not delete assets whose dynamic usage remains unresolved.

## Verification is part of implementation

Run semantic/token/source checks, the project build/type/lint/tests and browser
checks. Match viewport, device scale, zoom, fonts and scroll before visual
comparison; allow expected scale differences but investigate order-of-magnitude
errors. Test pseudo-state palettes, selected appearance, motion intermediate
states and data changes. Collect missing asset URLs, registry drift, opaque
filenames, exact duplicates and short-page footer gaps. Static validation, a
generated export, an API call in source or coincidentally matching initial data
does not establish runtime completion. Report verified and unverified scopes.

## Executable follow-up tools

- Apply exported motion controller/runtime files with semantic bindings while preserving generated server TSX. Follow behavior pagination until complete; preserve the final authenticated inventory.
- Use devup_source_sync plan/apply for a new generated design against an edited module. Review the plan hash and file preconditions; ambiguous topology or API-backed copy conflicts remain explicit and preserve current code. Node/DOM identities are never inferred from element positions after extraction.
- Use devup_assets inventory/plan/apply/rollback for public assets and exact source/seed references. Anonymous exports remain private and withhold public/code writes until semantically named. Persisted URL SQL is an artifact requiring separate execution evidence.
- Run devup_runtime_audit completion after real build/lint/typecheck/tests and bound browser/data audits. Generated source, wired implementation, browser verification and actual DB verification are distinct stages. Source changes invalidate stale receipts. Read the completion section in devup://guide/usage for the exact input and runner commands.
- The repository CI runs the complete browser/production App Router consumer gate on Linux through its existing checks.test.mjs entry. Rust fixture/visual binaries are prepared once before browser timers start; source fingerprints prevent stale binary reuse.

## Required page, API and verification boundaries

- Route pages must contain their actual page structure and data preparation in
  `app/**/page.tsx`. `components/pages/<page>/` contains page-local partials only.
  Never delegate the entire page to one component, including through artificial
  wrappers, fragments, providers or a shared header around a page proxy.
- Every application API call must use devup-api, including server reads, server
  actions and uploads. Native `fetch`, another HTTP client and custom transports
  are not substitutes. Before reporting a missing capability, use DCU to update
  the relevant dependencies, reinstall, verify the resolved versions, and rerun
  a minimal reproduction. Preserve the real API/schema contract, including null.
  If the required capability is still missing, document a concrete devup-api
  feature request and stop the affected implementation. Continue independent
  authorized page work. Posting externally requires explicit authorization.
- Run functional E2E and Figma reference visual tests together for UI changes.
  Do not create expected screenshots from the implementation under test. Cover
  every changed section at desktop/mobile widths and captured error, selected,
  expanded and disabled states. A passing initial screenshot is insufficient.
  Keep the actual, Figma reference, diff, threshold and renderer/font/zoom context.
- Diagnosis and feedback files do not complete an authorized fix. Apply the fix,
  rerun both kinds of browser verification, and report any remaining scope.
  A missing API, reference or render environment stays explicitly unverified.

## Assembly must preserve visible geometry

- A detached component needs its caller allocation as well as its internal
  styles. Apply `instanceOverrides.generatedProps` and
  `DEVUP_CODEGEN_INSTANCE_PLACEMENT_CONTRACT.requiredRootProps` to the actual
  typed component root, or inline the captured instance. `maxW` alone does not
  allocate FILL width. Do not silently discard root props when replacing a
  reference with a separately exported component. Measure both siblings in their
  assembled parent (Prime: 420px title, 40px gap, 520px cards in a 980px row).
- Preserve painted empty nodes. A childless absolute Box may draw a progress
  rule, dot, background, border, mask, image or effect. Never hide/delete it
  using only its position, width, tag or lack of text. Check paint, visibility,
  opacity and captured source identity first. Measure tiny decorations too.
- Semantic `h1`/`h2`/`h3`, native inputs and buttons can change browser defaults.
  Verify/reset computed margins, appearance and box sizing. Test input bounds,
  focus and ARIA before and after validation messages. Shared button wrappers
  must preserve the caller's width and alignment in every usage.
- A validation message added beside a native input must not take its width:
  `flex="1"` becomes `flex-basis: 0%` and a 100%-basis message can collapse the
  input to 0px. Keep an explicit width contract (input `flexBasis`/`minW`,
  message on its own row). E2E must trigger the real error, then assert input
  bounds (width > 0), visibility, focus and the message text in that state.
- Compare blur/crop/blend against the composed backdrop; a negative z-index or
  isolation can change multiply into white. Preserve intentional baked effects
  explicitly and retain pixel evidence for lossy image/absolute fallbacks.
- Resolve assets by source node/paint/asset identity, never the first matching
  placeholder filename. Keep collisions blocked. Validate tokens against the
  final assembled JSX, including instance exports, rather than the page alone.
  Distinguish hidden library masters from visible instances and overrides.
- Source/AST preservation, compiler compatibility, behavior, geometry and pixels
  are separate checks. Run the installed Devup UI transform plus real build;
  do not cast away compiler failures. Diagnose generated-CSS HMR reloads with
  server logs and a production render before changing unrelated page geometry.
- Large resources need bounded chunk/hash recovery; asset acquisition, public
  path projection and actual file writes are separate evidence. Check path,
  bytes, digest and blocked-write reason; do not treat a response as a saved file.
- Intentional backend-dependent design exceptions (such as unavailable CAPTCHA)
  must be listed separately from unrelated layout regressions. Do not invent a
  working control or claim API persistence from local state.

## Dependency and delivery evidence

Verify latest resolved package versions and a minimal reproducer before declaring
an upstream capability missing. Use the exact CI runtime, restore global mocks,
check exit status and required coverage, and verify remote checks after pushing.
PR creation, merge and release publication are distinct outcomes. For release
work use a clean fixture, the actual package archive paths and archive contents
(name, version and internal dependencies). Never accept stale tarballs, dry-run
output or a retry of a pinned old release source as proof of a current release.

Report a CI failure by the stage that actually failed (setup, install, build,
typecheck, unit, E2E) with its original message. Local pass, remote CI pass, a
push/permission rejection and a transient external HTTP error (for example
`setup-bun` returning 504 before any build ran) are separate evidence; one
never stands in for another. When a push is rejected, finish the independent
work, leave an applicable patch (`git apply --check`), state the constraint and
do not bypass it with other credentials.

A dependency minor bump can enable new accessibility rules: axe-core 4.14
promoted `label-content-name-mismatch` (WCAG 2.5.3 Label in Name) to wcag21a.
Visible text includes `aria-hidden` split-letter animation copies, so an
`aria-label` that merely repeats the button text fails. Drop redundant
`aria-label`s (the name then comes from content minus `aria-hidden` parts);
make any remaining label contain the visible text verbatim ("1 페이지", not
"1페이지"; "KOR, 언어 선택"); put extra hints such as "(새 창에서 열림)" in
sr-only text. Fix the components; never disable or downgrade the rule.

## Concurrent projects and split-screen desktop

Another running project is not a desktop-wide lock. When authorized projects
need GUI work, use OS split-screen with separate visible Figma/browser windows
and continue them side by side. Keep each project's workspace, terminal, server
port and browser context distinct. Do not close, minimize or replace another
project's working window, server or Bridge plugin to make room.

Before each input or capture, verify the target window identity and bounds,
plus its URL/port or Figma file/page. Recheck coordinates after tiling. Coordinate
only conflicting mouse/keyboard input in short batches, not the entire project.
A real tool lock or permission error blocks its affected operation; report that
exact error, respect the lock and continue independent work. Do not invent a
single-project restriction or claim to bypass a tool-enforced lock.

Keep functional E2E and Figma visual tests paired. Desktop pane size does not
replace the agreed CSS viewport, DPR or zoom. If the required capture viewport
cannot fit onscreen, use an independent automated browser context at that size;
do not change application layout, CSS zoom or thresholds to make a pane match.

## Final report: offer the retrospective

When the work is complete, end the final completion report with one question
offering the session retrospective, in the user's language:

- Korean: "작업이 완료되었습니다. 지금까지 진행한 내용을 개선하기 위해 /devup-retro 를 실행할까요?"
- otherwise: "The work is complete. Shall I run /devup-retro to turn this session's follow-up prompts into devup-mcp improvements?"

Use the invocation that works in this client: `/devup-retro` when the
`devup-retro` skill is installed, otherwise the MCP prompt form
`/mcp__<server>__devup-retro`. A tool result that marks completion carries the
ready sentence as `nextSuggestedAction` (`devup_runtime_audit` completion with
`reportAllowed`, `devup_screen_plan` status with `completion.reportAllowed`,
`devup_stack_diff` with every layer checked and zero drift). An export's
`deliverable.isFinal` grades generated source and carries no offer.

Ask once per completed task. Not after an intermediate tool call, a failed or
unresolved result, or a partial report, and not again once asked. Never run it
yourself: it starts only when the user says yes.

## Contract identifiers

Export replies carry `developmentContract` as identifiers only, to stay inside
the reply's byte budget; this section is what they mean. Read these rules,
apply `behaviorPlan`, fix and rerun functional E2E plus native Figma visuals
before claiming completion. The installed copy of this document is
`devfive-frontend/references/delivery-quality.md`.

`requirements`:

| Identifier | Requires |
|------------|----------|
| `pure-tsx-sidecar-provenance` | Application TSX carries no `@devup-*` comments or Figma node attributes; provenance lives in JSON sidecars |
| `jsx-theme-tokens-and-axis-shorthands` | Real `$token` props in JSX, no `var(--token)`; equal pairs collapse to `px`/`py` |
| `single-responsive-tree-and-source-text-wrapping` | One tree with responsive arrays for the same content; Korean wrapping (`keep-all`) and captured line breaks preserved |
| `persistent-server-layout` | Header, tabs and footer live in the nearest shared server `layout.tsx` and stay mounted across routes |
| `route-owned-page-body` | Each route owns its page body; screens are routes, not local state |
| `devup-api-only` | Data goes through devup-api, never an ad-hoc fetch or fixture |
| `stop-affected-api-gaps` | A missing or wrong API stops the affected work and is reported, never papered over in the UI |
| `preserve-instance-sizing-and-paint` | Captured instance overrides, sizes and paints are kept |
| `paired-e2e-and-native-figma-visuals` | Functional E2E and native Figma visual comparisons are run together |
| `semantic-native-controls-and-complete-pseudo-states` | Native elements with accessible names and every pseudo-state painted |
| `variant-palette-consistency` | Every variant uses the captured palette, including selected and disabled |
| `single-source-url-content-selection` | The URL is the single source of which content is selected (`aria-current`) |
| `viewport-footer-shell` | A short page keeps its footer at the viewport bottom |
| `motion-candidates-and-safe-progressive-enhancement` | Each `behaviorPlan` motion candidate (marquee, count-up, step cycle, sliding highlight, section entrance) is implemented as an enhancement of readable server markup |
| `default-entrance-and-transition-motion` | Restrained entrance and transition motion is proposed by role and implemented without being asked; the user's stated design wins |
| `devup-ui-static-keyframes-motion` | Motion is Devup UI `keyframes()` and CSS animation props extracted to static CSS; no animation library for entrances or transitions |
| `one-shot-reveals-without-ssr-flash` | A reveal completes when its content is visible and never replays, re-hides or flashes on hydration, reload, back/forward or client navigation |
| `canonical-semantic-assets` | One asset registry, semantic file names, no duplicates or hash names |
| `landing-detail-api-db-single-source` | Landing and detail values come from the same API and DB records |
| `runtime-evidence-before-completion` | Completion is claimed from a runtime audit, not from the export or a screenshot |
| `offer-devup-retro-in-final-report` | The final completion report ends by offering `/devup-retro` once, in the user's language; it is never run unasked |

`completionEvidence`:

| Identifier | Evidence |
|------------|----------|
| `both-control-directions-and-history` | Every control exercised both ways, plus back/forward and reload |
| `all-widths-and-zoom-context` | Every agreed width, with viewport, device scale and zoom recorded |
| `computed-style-and-animation-intermediate-state` | Computed styles of states and an intermediate value of every running motion (marquee, count-up, steps, tabs), not only the final frame |
| `animation-start-middle-end-frames` | The start, middle and end frame of each entrance, as measured opacity/transform values with a screenshot of each, at a mobile and a desktop width |
| `motion-trigger-reentry-history-and-image-failure` | Trigger position, re-entry, fast scroll, reload, history, bfcache, client navigation, delayed and failed images, focus, reduced motion and no JavaScript for each reveal |
| `stagger-total-and-progress-sync-timing` | For a group: stagger step, span and total time within bounds; for a progress line: each item within tolerance of the line reaching it |
| `changed-empty-and-failing-api` | Changed, empty and failing API states, against a disposable database |
| `all-public-assets-and-persisted-url-references` | Every public asset and every persisted URL reference resolves |

Standing policies of every `behaviorPlan` (they used to be repeated in each
reply): captured prototype evidence and inferred motion choices remain
separate, and explicit user requirements take precedence. Keep generated
server TSX and data pages, and mount the supplied tiny shared motion controller
in the persistent layout. Use semantic selectors; node identities and
implementation evidence stay in JSON sidecars. The immutable artifact graph and
fingerprint are read for context, while candidate inference and returned rows
are bounded per page (`evidenceLimits`).
