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
- Korean sentences use `wordBreak="keep-all"` and normal wrapping. Do not fix
  word splitting with `nowrap`; handle long URLs or single tokens explicitly.

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
