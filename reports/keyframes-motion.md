# Devup UI keyframes motion, runtime motion checks and export budget

Follow-up to `chondrogen-motion-followups.md` and PR #124. It closes the items
that PR left as limitations and moves the recommended and generated motion
from a script-driven entrance to Devup UI's static `keyframes()`.

## 1. Motion is Devup UI static keyframes

### The API, confirmed by compiling it

Compiled with the versions the harness installs (`@devup-ui/react` 1.0.41,
`@devup-ui/vite-plugin` 1.0.73, stylesheet header `devup-ui v1.0.83`) and read
back from the emitted CSS:

| Source | Emitted |
|--------|---------|
| `const rise = keyframes({ from: {…}, to: {…} })` | `@keyframes a-a{from{…}to{…}}`; the const is the name |
| `animationName={rise}`, `animation={`${rise} 400ms ease-out both`}` | `animation-name:a-a`, `animation:a-a 400ms ease-out both` |
| `animationTimeline="view()"`, `animationRange="entry 0% entry min(100%, 240px)"` | `animation-timeline:view()`, `animation-range:entry 0 entry min(100%,240px)` |
| `animationDelay={`calc(${index} * 60ms)`}` | a class with `animation-delay:var(--a)` and the value in `style` |
| `_motionReduce={{ animation: 'none' }}` | `@media(prefers-reduced-motion:reduce){…{animation:none}}` |
| `_focusWithin={{ animation: 'none' }}` | `…:focus-within{animation:none}` |
| `selectors={{ '&[data-reveal-state="in"]': {…} }}`, `'[data-reveal-state="in"] &'` | `.x[data-reveal-state="in"]{…}`, `[data-reveal-state="in"] .x{…}` |

A wrapper component does not forward Devup UI style props: they are extracted
where they are written. `Reveal` therefore takes plain element props and
layout goes on a `Box` inside it.

### What ships

- `templates/Reveal.tsx` (new export output `revealTsx`, resource
  `Reveal.tsx`, delivered with every section-entrance candidate): `Reveal`
  (scroll-linked section), `RevealGroup` + `RevealItem` (staggered group on a
  clock), `RevealLine` + `RevealStep` (progress line and steps timed from one
  variable). Server code, no `'use client'`, no animation library.
- `templates/motion-runtime.ts`: the section-entrance path no longer animates
  anything. It sets `data-reveal-state` once, only for `data-reveal="timed"`
  groups and for browsers without view timelines, only on blocks still below
  the viewport when it attaches, never under reduced motion, and removes its
  marks on unmount. Focus inside a marked block shows it at once.
- `behaviorPlan` section-entrance candidates carry
  `pattern: "devup-ui-static-keyframes-reveal"` and the keyframes
  implementation; `motionBindings.entranceStyle` is
  `devup-ui-static-keyframes`.
- `animation.md` is rewritten keyframes-first (why: no bundle, no hydration
  flash, compositor-only properties), with framer-motion limited to a project
  that already depends on it and a user who asks, or to exit and gesture
  motion. `delivery-quality.md`, `SKILL.md`, `common-patterns.md` and
  `critical-rules.md` follow.

### A range finding worth keeping

`animation-range: entry 0% entry 100%` is right for a block shorter than the
screen and wrong for a taller one: measured at 390x844, a 1200px section is
28% opaque with 240px of it showing and reaches 100% only when its top reaches
the top of the screen. `entry 0% entry min(100%, 240px)` completes after 240px
or when the block is fully in view, whichever comes first. That is the shipped
range, and the audit fails a scroll-linked block that is not complete with
half a screen of it showing.

### Static validation reads both styles

`motion_quality.rs` rules now read Devup UI keyframes (JSX props and
`css()`/`styled` objects, `keyframes()` constants and inline calls) as well as
framer-motion, in a new module `motion_keyframes.rs`:

| Rule | Keyframes form that is reported | Stays silent |
|------|--------------------------------|--------------|
| `ssr-hidden-reveal` | entrance on an element that itself rests at `opacity: 0`/`visibility: hidden`; entrance paused on its hidden first frame; hidden until a selector on a script-set attribute/class shows it | hidden start only in the keyframes; hidden only under a script-set `pending` marker; hover overlays; client-mounted elements |
| `reveal-replays` | `animationTimeline="view()"` with no range ending inside the entry (`cover`, `exit`, none, above `cover 50%`); an `IntersectionObserver` callback that writes `isIntersecting` into the element | `entry …` ranges; observers that `unobserve`/`disconnect`; `scroll()` progress bars; callbacks declared elsewhere |
| `reveal-moves-background` | the element carrying the entrance paints a background (props or `css()` class, either way round) | paint outside and motion inside; looping loaders; keyframes from another module |
| `stagger-too-long` | `animationDelay`/`transitionDelay` with a per-item step above 150ms (`calc(var(--i) * step)`, template `index * step`) | steps up to 150ms; one fixed delay |
| `motion-without-reduced-motion` | a keyframes entrance with no `_motionReduce`/`_motionSafe`/media rule in the validated sources | loaders, hover transitions |

New rules: `entrance-uses-motion-library` (info, once per file),
`entrance-on-static-content` (nav, header, footer, form, fields, body copy)
and `entrance-per-element` (info: a heading or control animated on its own).
The last two turn the role table of the guide into checks.

## 2. Export reply budget

`export_context_summarizes_a_large_project_under_fifteen_kib`: 15,255 bytes
before (105 of headroom after #124), 14,086 after: 1,274 bytes of headroom,
with a test that fails below 1,024.

Nothing was dropped. Standing text that every reply repeated now lives in the
guide the reply already points at, and a test asserts each moved sentence is
there:

| Moved to the guide | Bytes |
|--------------------|-------|
| `behaviorPlan.intentPolicy`, `clientBoundary`, `sourcePurity`, `evidenceLimits.contextIndex` | ~540 |
| `developmentContract.nextAction`, `installedReference` | ~230 |
| `verificationScope` explanations (now field names plus a guide pointer) | ~300 |
| `server.updateAvailable.autoUpdate.installedPathNote` | ~310 |

`developmentContract` (version 4) keeps every identifier inline and gains
four. The two that #124 renamed are back under their own names, next to the
new ones: `motion-candidates-and-safe-progressive-enhancement`,
`default-entrance-and-transition-motion`, `devup-ui-static-keyframes-motion`,
`one-shot-reveals-without-ssr-flash` (20 requirements) and
`computed-style-and-animation-intermediate-state`,
`animation-start-middle-end-frames`,
`motion-trigger-reentry-history-and-image-failure`,
`stagger-total-and-progress-sync-timing` (8 evidence items).
`delivery-quality.md` has a "Contract identifiers" section explaining all 28,
checked by `every_contract_identifier_is_explained_in_the_guide`.

## 3. Runtime motion checks

`harness/runtime-audit/reveal-checks.mjs` (new; `motion-checks.mjs` delegates
`kind: "reveal"` to it). It measures an entrance whatever produced it and
distinguishes a scroll-linked one (a view timeline: state follows position)
from a timed one.

Always measured: start, middle and end frames (opacity/transform values and a
screenshot each), trigger position, re-entry. Then twelve scenarios, each from
a fresh load, all of them by default:

| Scenario | Passing page | Failing page |
|----------|--------------|--------------|
| trigger position | starts within 1px of the viewport | observer with a 600px margin (early); 900ms timer (late) |
| scroll-linked completeness | `entry 0% entry min(100%, 240px)` | no range (`cover`); plain `entry 100%` on a 1200px block |
| re-entry | one-shot | replays; re-hides while away; page too short to leave |
| `fast-scroll` | shown after a fling | stays at opacity 0 |
| `reload`, `history` | shown at the restored position | replays when already on screen |
| `bfcache` | shown after a real restore | replays on `pageshow` with `persisted` |
| `client-nav` | same-document route change and back | replays on return |
| `image-delay`, `image-failure` | reveals at the reserved height | reveal gated on the image `load` event |
| `focus` | focused control painted, on screen, Tab leaves | animates a focused block; no `:focus-within` rule on a landscape screen |
| `reduced-motion` | in place while entering | ignores the preference; no media rule |
| `no-script` | present and painted | `opacity: 0` in markup until a script runs |
| `stagger` | 60ms steps, 5 items | 250ms steps; 1600ms items |
| `progress-sync` | each step within 200ms of the line | all steps at once |

`bfcache` uses a real restore: Playwright starts Chromium with the cache off,
so the scenario starts full Chromium with `--disable-back-forward-cache`
removed and requires `pageshow` to report `persisted`.

Results are `pass`, `fail`, `not-applicable` (nothing in the block to observe)
or `unverified` (could not be observed; the check is then `inconclusive`). An
explicit `scenarios` list makes an unobservable scenario a failure and names
the rest in `measured.scenariosNotRun`.

`devup_runtime_audit` ingest returns `motionEvidence`: a row per motion check
per width and a line per candidate, `verified` only with a passing check at a
mobile (<=767px) and a desktop (>=1024px) width and nothing left unverified.
Frame screenshots are accepted only when their bytes match the report.

The runner no longer treats a view-timeline animation as a running clock: the
settle step skipped waiting 15 seconds for it, and the freeze step cancels it
(the resting style is the content) instead of pausing it at its hidden first
frame, which would have blanked every below-fold section in captures.

## 4. Monorepo single-`tsx` validation

`ui_validate_owner.rs` resolves the owning app's `devup.json` for one `tsx`:

1. `sourceName` as a path inside `projectRoot`, relative or absolute (short
   8.3 names and symlinks are resolved); the file need not exist.
2. `sourceName` as the tail of a path (`src/Hero.tsx`, `Hero.tsx`): project
   files ending with it, when one theme governs them all.
3. No usable name: project files whose text is this `tsx` (line ends and
   surrounding blank space ignored), when one theme governs them all.

`tokens.owner` says which rule decided (`sourceName`, `sourceName-suffix`,
`content-match`). Unverified is reported only when no owner resolves: no
match, a file no theme governs, or matches under different themes, and then
`themeGuardrail.ownerCandidates` names what was found. The search is bounded
(20,000 directory entries, 256 files opened, depth 12) and never leaves
`projectRoot`.

## Unverified is never a pass

- `devup_runtime_audit`: the verdict is `pass` only when every planned item
  passed. A reveal check with an unobserved scenario is `inconclusive`, which
  is counted under `summary.inconclusive`, never under `summary.pass`, and
  makes the verdict `inconclusive`. `motionEvidence` counts a candidate as
  `verified` only with passing mobile and desktop checks and an empty
  `unverified` list; completion applies the same rule
  (`a_reveal_candidate_needs_passing_checks_at_mobile_and_desktop`).
- `devup_ui_validate`: token references that were not checked make
  `okReason` `clean-unverified-tokens` instead of `clean`, with
  `themeAvailable` and `tokens.unknownTokenCheckRan` false; a partly checked
  run is `tokens.verification: "partial"`.

## What stays with a person

All machine-checkable limitations listed in #124 are resolved above. Whether
an eligible content block should get motion at all is a user design choice
(user-specified design wins); the tool proposes, it does not judge.

## Regression evidence

- Rust: 24 new tests (2,286 -> 2,310 passing). `motion_keyframes_tests.rs` (9, one per rule in
  keyframes form with true negatives), `motion_keyframes.rs` (2), contract and
  guide (1 new, 1 extended), runtime audit plan validation (extended) and
  `motion_evidence.rs` (3), owner resolution unit (2) and MCP integration (2:
  nine resolved cases across `apps/a` and `apps/b`, five unresolved ones),
  behavior plan keyframes output (extended), export budget and width policy
  (extended).
- Browser (Chromium 153): `keyframes-motion.test.mjs` compiles the shipped
  templates with Devup UI, prerenders the markup from the same bundle and
  runs every check at 390x844 and 1440x900 (scroll-linked section, the same
  section through the marker fallback, a card group, a timeline);
  `reveal.test.mjs` holds the pages above, six tests.
