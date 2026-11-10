# Entrance and transition motion

Motion is a default quality item, not an extra. Nobody has to ask for it: on
every page you build or change, run the pass below, propose the result, then
implement it. A design the user stated wins over every default here — an
explicit "no animation", a specified easing or duration, a replaying effect, a
captured prototype. Record such a choice and do not second-guess it.

## 1. Survey the page, then propose by role

Walk the page's sections and repeated content before writing any motion.
Choose by what a block is for, not by what can be animated:

| Block | Default |
|-------|---------|
| First viewport (hero, page title) | No scroll reveal. It is visible on load; at most one short settle that never starts hidden in server markup |
| Section below the fold | One restrained entrance for the section's content group: short fade with a small upward move |
| Card grid, list, logo row, steps | The group enters together or with a short stagger (40-80ms per item, capped so the whole group finishes within about 400ms) |
| Section with a painted background, image or band | The painted element stays fixed; an inner content wrapper moves |
| Progress line, timeline, chart, graph | The line/graph advances in step with the items it connects: an item appears as the line reaches it, not before and not long after |
| State change (tab, accordion, hover, selected) | A short transition on the properties that change |
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

## 2. Scroll reveals fire when content is visible, once

- Start when the content is actually in view, not when its container's top
  edge touches the viewport and not at page load for content far below. Use a
  visibility amount or margin that fits the block's height; a tall section
  needs a small amount, or it never fires on a short screen.
- Play once. Re-entering the viewport, scrolling back up or returning with
  back/forward must not replay or re-hide content
  (`viewport={{ once: true }}`, or an observer that unobserves after firing).
- Stagger card groups only when it helps reading order, and keep it short.
  Derive the delay from the item index, never from a hardcoded list length.
- A section with a background keeps that background still. Put the trigger
  and the transform on an inner wrapper so no band, image or colour fades or
  slides against the page.
- A progress line or graph shares one trigger and one timeline with its
  items. Derive item delays from the same duration as the line so the two
  cannot drift when either changes.
- A fast scroll past several sections must leave every passed section fully
  visible, not stuck mid-animation or still hidden.

## 3. No flash, no blank, no dependency on JavaScript

Server markup is the content. It must be readable before, during and without
hydration.

- Never ship server HTML whose content starts at `opacity: 0` or translated
  away and waits for JavaScript to show it. `initial={{ opacity: 0 }}` on an
  always-rendered `motion.*` element does exactly that: it is invisible until
  hydration, and stays invisible when the script is slow, blocked or fails.
- Equally, do not render visible content and then hide it on hydration to
  replay an entrance: that is the flash. Decide the hidden start only for
  content that is still below the viewport when the enhancer attaches;
  content already in view stays as rendered.
- Reload, direct load, client navigation and back/forward all start from
  visible content. A restored scroll position must not leave the visible
  sections blank until the user scrolls.
- With JavaScript disabled or still loading, every section is visible and
  usable. Prefer one shared observer/controller that enhances server markup
  over many client components that each own visibility.
- `prefers-reduced-motion: reduce` shows content in place with no movement.
  Handle it once (`<MotionConfig reducedMotion="user">`, `useReducedMotion()`,
  or a media rule) and verify it.
- Keyboard focus is never hidden or moved by an entrance. An element that
  receives focus is shown immediately in its final position; do not animate a
  focused control or trap focus in a block that has not revealed yet.
- Images inside a revealed block reserve their size. A slow or failed image
  must not hold the block hidden or shift it when it arrives.

## 4. Verify the motion, not the final screenshot

A correct final frame says nothing about the animation. For every motion you
added or changed, at one mobile and one desktop width:

- **Frames**: capture or measure the start, a middle and the end state
  (opacity/transform values, not only pixels). The start must be the visible
  server state, the middle must really be intermediate, the end must match
  the design.
- **Trigger timing**: record where the block was relative to the viewport
  when it started. "Starts while still off screen" and "starts after the user
  is already reading it" both fail.
- **Re-entry**: scroll away and back; nothing replays or re-hides.
- **Fast scroll**: jump or fling to the bottom; every passed block is fully
  visible.
- **History**: go to another route and come back with back/forward, and
  reload mid-page; no blank or flashing section at the restored position.
- **Images**: throttle images and block one; the block still reveals, at the
  reserved size.
- **Preferences and access**: reduced motion, JavaScript disabled, and
  Tab/Shift+Tab through the block's controls before and during its entrance.

`devup_runtime_audit` motion checks (`kind: "reveal"`) measure the
intermediate frame, the final state and re-entry for a bound target.
`devup_ui_validate` reports what the source alone proves:
`ssr-hidden-reveal`, `reveal-replays`, `reveal-moves-background` and
`motion-without-reduced-motion`. Neither replaces the list above. Report each
item as verified or explicitly unverified, with the width it was checked at;
never report motion as done from a final screenshot.
