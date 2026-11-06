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
- Preserve Figma line breaks and measured wrapping at each captured width.
  Do not apply `wordBreak="keep-all"` solely because text is Korean. Explicit
  line breaks, font metrics and available width take precedence over a language
  heuristic; verify automatic wrapping against the native Figma reference.

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
- Use restrained below-to-above entrance motion for meaningful sections. One
  shared observer/controller can enhance server markup, including sections added
  after navigation. Do not hide SSR content before JavaScript runs. Support reduced
  motion, no-JavaScript, focus, one-time playback and observer cleanup.
- Add transitions for captured colour, border and state changes without animating
  every layout property. Reduced-motion behavior is the accessible default unless
  an explicit requirement says otherwise; document intentional exceptions.

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
