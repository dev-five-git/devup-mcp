# Entrance and transition motion

Motion is a default quality item, not an extra. Nobody has to ask for it: on
every page you build or change, run the pass below, propose the result, then
implement it. A design the user stated wins over every default here — an
explicit "no animation", a specified easing or duration, a replaying effect, a
captured prototype. Record such a choice and do not second-guess it.

Motion is written with Devup UI's static `keyframes()` and CSS animation
props. The build extracts them to a stylesheet, so an entrance costs no
JavaScript bundle, cannot flash on hydration (there is nothing to hydrate) and
runs on the compositor (`opacity` and `transform` only). Do not add
framer-motion or another animation library for entrances or transitions. Use
one only when the project already depends on it **and** the user asks for it,
or for what CSS cannot express (gesture-driven or physics motion, exit
animations of unmounting elements, shared-layout transitions); say so when you
do.

## 1. Survey the page, then propose by role

Walk the page's sections and repeated content before writing any motion.
Choose by what a block is for, not by what can be animated:

| Block | Default |
|-------|---------|
| First viewport (hero, page title) | No scroll reveal. It is visible on load; at most one short settle that never starts hidden in server markup |
| Section below the fold | One restrained entrance for the section's content group: short fade with a small upward move |
| Card grid, list, logo row, steps | The group enters together or with a short stagger (40-80ms per item, capped so the whole group finishes within about 400ms of stagger and one second in total) |
| Section with a painted background, image or band | The painted element stays fixed; an inner content wrapper moves |
| Progress line, timeline, chart, graph | The line/graph advances in step with the items it connects: an item appears as the line reaches it, not before and not long after |
| State change (tab, accordion, hover, selected) | A short `transition` on the properties that change |
| Body copy, form fields, navigation, footer, legal text | None |

Restraint is part of the rule. Do not animate every heading or every control,
do not chain a title, subtitle and button as three separate reveals, and do not
add motion to something the user reads or operates immediately. One trigger
per content group is the normal case. Durations stay around 200-500ms with an
ease-out curve; distances stay small (8-24px).

Tell the user what you propose, section by section, in the first response
that covers the page (a short table is enough: block, motion, trigger, why).
Continue with the implementation unless they object; do not wait for approval
of defaults.

## 2. Implement it as static keyframes

`devup_figma_export` hands out `Reveal.tsx` with every section-entrance
candidate (`behaviorPlan.motionImplementations.files.revealTsx`); the same
code is below. Declare the keyframes once per project, in the file that uses
them: a `const` holding a `keyframes()` name is a build-time value only in its
own file.

### A section that enters as it scrolls into view

```tsx
import { Box, keyframes } from '@devup-ui/react'

// The hidden start exists only here. Markup without a running animation is
// the visible content.
const rise = keyframes({
  from: { opacity: 0, transform: 'translateY(16px)' },
  to: { opacity: 1, transform: 'translateY(0)' },
})

// The painted section stays still; the wrapper inside it moves.
<Box as="section" bg="$surface">
  <Box
    data-reveal=""
    animationName={rise}
    animationTimingFunction="ease-out"
    animationFillMode="both"
    animationTimeline="view()"
    animationRange="entry 0% entry min(100%, 240px)"
    _motionReduce={{ animation: 'none' }}
    _focusWithin={{ animation: 'none' }}
    selectors={{
      '&[data-reveal-state="pending"]': { animationName: 'none', opacity: 0 },
      '&[data-reveal-state="in"]': { animationTimeline: 'auto', animationDuration: '400ms' },
    }}
  >
    …content…
  </Box>
</Box>
```

What each line is for:

- `animationTimeline="view()"` binds the keyframes to the block's own position
  in the viewport. There is no clock and no trigger: the state is a function
  of scroll position, so it cannot start late, replay, or be left half done by
  a fast scroll, a reload or back/forward.
- `animationRange="entry 0% entry min(100%, 240px)"` ends the entrance once
  the block is fully in view or 240px in, whichever comes first. Without a
  range (or with `cover`/`exit`) the block is still part-transparent in the
  middle of the screen and fades out again as it leaves; a plain
  `entry 100%` leaves a section taller than the screen half faded while it is
  being read.
- `animationFillMode="both"` holds the first frame before the range and the
  last one after it. The resting style has no `opacity: 0`: never put the
  hidden start on the element itself.
- `_motionReduce={{ animation: 'none' }}` shows the content in place for
  `prefers-reduced-motion`. `_focusWithin={{ animation: 'none' }}` shows it at
  once when a control inside it receives keyboard focus, so focus is never
  inside a part-transparent block.
- Where view timelines are unsupported the two timeline properties are
  dropped, the animation has no duration and the content is simply shown: the
  base style is safe everywhere, with or without JavaScript.
- The two `selectors` are the progressive enhancement for those browsers. The
  shared `MotionController` (`motion-runtime.ts`, a few lines with no
  animation code) sets `data-reveal-state="pending"` only on blocks that are
  still below the viewport when it attaches, and `"in"` once when a block
  enters (one `IntersectionObserver` that unobserves after firing). It never
  hides anything that is already on screen, never runs under reduced motion,
  and removes its marks when it unmounts. Bind it with
  `{ kind: 'section-entrance', target: '[data-reveal]' }`.

Layout for the revealed content goes on a Box inside the wrapper: Devup UI
extracts style props where they are written, so a shared `<Reveal>` component
takes plain element props, not style props.

### A group on a clock: staggered cards, a progress line

A stagger and a line that draws over time need one trigger and a clock, which
a view timeline does not have. Mark the group `data-reveal="timed"`: the same
controller sets its state once when it enters, in every browser, and the
items animate from that:

```tsx
<Box data-reveal="timed" display="flex" gap="16px">
  {cards.map((card, index) => (
    <Box
      key={card.id}
      style={{ '--reveal-index': index }}
      _motionReduce={{ animation: 'none' }}
      selectors={{
        '[data-reveal-state="pending"] &': { opacity: 0 },
        '[data-reveal-state="in"] &': {
          animationName: rise,
          animationDuration: '400ms',
          animationTimingFunction: 'ease-out',
          animationFillMode: 'both',
          animationDelay: 'calc(var(--reveal-index) * 60ms)',
        },
      }}
    >
      …
    </Box>
  ))}
</Box>
```

- Derive the delay from the item index (`--reveal-index`), never from a
  hardcoded list length, and keep the step at 40-80ms.
- A progress line or graph shares one variable with its items. The line draws
  over `--reveal-line` and step `i` of `n` waits
  `calc(var(--reveal-line) * var(--reveal-index) / var(--reveal-count))`, so
  the two cannot drift when either changes (`RevealLine` and `RevealStep` in
  `Reveal.tsx`).
- With no script the group has no state and every item is shown.

### State changes

A tab, accordion, hover or selected state is a `transition` on the properties
that change (`transition="opacity .2s, transform .2s"`), switched off with
`_motionReduce={{ transition: 'none' }}`. Do not animate layout properties.

## 3. Rules the implementation must keep

- **Visible when in view, once.** A scroll-linked entrance completes inside
  the entry and stays complete for as long as the block is on screen or has
  been passed. A timed one starts when the block enters (not at page load for
  content far below, and not after the reader is already looking at it) and
  never restarts: re-entering the viewport, scrolling back up, reload and
  back/forward must not replay or re-hide content.
- **The background stays still.** Put the animation on an inner wrapper so no
  band, image or colour fades or slides against the page.
- **Server markup is the content.** It is readable before, during and without
  hydration: nothing ships at `opacity: 0` or translated away waiting for
  JavaScript, and nothing visible is hidden on hydration to replay an
  entrance. Reload, direct load, client navigation, bfcache and a restored
  scroll position all start from visible content.
- **Fast scroll.** A jump past several sections leaves every passed section
  fully visible, not stuck mid-animation or still hidden.
- **Preferences and access.** `prefers-reduced-motion: reduce` shows content
  in place with no movement. Keyboard focus is never hidden or moved by an
  entrance, and never trapped in a block that has not revealed yet.
- **Images.** Images inside a revealed block reserve their size. A slow or
  failed image must not hold the block hidden or shift it when it arrives.

## 4. Verify the motion, not the final screenshot

A correct final frame says nothing about the animation. For every motion you
added or changed, at one mobile and one desktop width:

- **Frames**: measure the start, a middle and the end state (opacity and
  transform values, not only pixels). The middle must really be intermediate
  and the end must match the design.
- **Trigger timing**: record where the block was relative to the viewport
  when it started. "Starts while still off screen" and "starts after the user
  is already reading it" both fail.
- **Re-entry**: scroll away and back; nothing replays or re-hides.
- **Fast scroll**: jump or fling to the bottom; every passed block is fully
  visible.
- **History**: go to another route and come back with back/forward (a new
  document, a bfcache restore and a client navigation are three different
  paths), and reload mid-page; no blank or flashing section at the restored
  position.
- **Images**: throttle images and block one; the block still reveals, at the
  reserved size.
- **Preferences and access**: reduced motion, JavaScript disabled, and
  Tab/Shift+Tab through the block's controls before and during its entrance.
- **Groups**: the stagger step, the stagger span and the total time of a card
  group; each item of a progress line against the moment the line reaches it.

All of this is measured by a `devup_runtime_audit` motion check with
`kind: "reveal"`. `target` is the element that carries the entrance (the
inner wrapper, not the painted section); `items` names the animated children
of a group. The check always measures the three frames (and keeps a
screenshot of each), the trigger position and re-entry, and then runs every
scenario below from a fresh load unless `scenarios` narrows the list:

| Scenario | Fails when |
|----------|------------|
| `fast-scroll` | after a fling to the bottom the block is not fully shown |
| `reload`, `history` | the block is hidden or displaced at the restored scroll position |
| `bfcache` | the block flashes when the page is restored from the back/forward cache |
| `client-nav` | the block flashes after leaving by `navigate` (a link) and going back in the same document |
| `image-delay`, `image-failure` | a stalled or aborted image keeps the block hidden or changes its height |
| `focus` | a focused control inside the block is part-transparent, off screen or keeps focus trapped (`focus` selects the control) |
| `reduced-motion` | the block is hidden or moves while entering under `prefers-reduced-motion` |
| `no-script` | the block is missing or hidden in the page served without JavaScript |
| `stagger` | a step between `items` exceeds 150ms, the stagger span 450ms or the whole group 1000ms (`maxStepMs`, `maxSpanMs`, `maxTotalMs`) |
| `progress-sync` | an item appears more than 200ms before or after the line reaches it (`progress` and `items` selectors) |

Results are never rounded up. A scenario with nothing to observe in the block
(no image, no control, no items) is `not-applicable`. One that could not be
observed — no link named for `client-nav`, a page the browser will not keep in
bfcache, a scroll position that was not restored — is `unverified`, and the
check is then `inconclusive`, not passed; a scenario you list explicitly and
that cannot be observed fails. `devup_runtime_audit` ingest folds this into
`motionEvidence`: a candidate is `verified` only by a passing check at a
mobile and a desktop width with nothing left unverified.

`devup_ui_validate` reports what the source alone proves, for keyframes and
for a motion library alike: `ssr-hidden-reveal`, `reveal-replays`,
`reveal-moves-background`, `stagger-too-long`, `motion-without-reduced-motion`
and, as information, `entrance-uses-motion-library`.

Still a judgement, not a measurement: whether motion suits the block at all.
Report each item as verified or explicitly unverified, with the width it was
checked at; never report motion as done from a final screenshot.
