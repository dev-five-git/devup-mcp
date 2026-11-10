import { Box, keyframes } from '@devup-ui/react';
import type { ComponentProps } from 'react';

// The one entrance of the project. The hidden start exists only in these
// keyframes: markup without a running animation is the visible content, so
// nothing depends on JavaScript, hydration or a supported timeline to appear.
const rise = keyframes({
  from: { opacity: 0, transform: 'translateY(16px)' },
  to: { opacity: 1, transform: 'translateY(0)' },
});

// Plain element props: Devup UI extracts style props where they are written,
// so layout for the revealed content goes on a Box inside (or a css() class),
// not through this wrapper.
type RevealProps = ComponentProps<'div'>;

/**
 * A content group that enters as it scrolls into view, as static CSS.
 *
 * Put it inside the painted section, never on it: the background stays still
 * and this wrapper moves. The range ends once the group is fully in view or
 * 240px in, whichever comes first, so a tall group is not read half faded. Where view timelines are unsupported the shared
 * MotionController marks the group once (`data-reveal-state`) and the same
 * keyframes run for 400ms; with no script at all the content is simply shown.
 */
export function Reveal(props: RevealProps) {
  return (
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
      {...props}
    />
  );
}

/**
 * A group whose items enter one after another on a clock: cards, list rows,
 * steps along a progress line. One trigger for the whole group (the shared
 * MotionController marks it once when it enters), items delayed by their
 * index. Give each item `<RevealItem index={i}>`.
 */
export function RevealGroup(props: RevealProps) {
  return <Box data-reveal="timed" {...props} />;
}

/** One item of a `RevealGroup`: waits `index * 60ms`, then rises. */
export function RevealItem({ index, style, ...props }: RevealProps & { index: number }) {
  return (
    <Box
      style={{ ...style, '--reveal-index': index }}
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
      {...props}
    />
  );
}

const draw = keyframes({
  from: { scale: 'var(--reveal-line-from, 1 0)' },
  to: { scale: '1 1' },
});

/**
 * The progress line of a timeline inside a `RevealGroup`. It is drawn over
 * `--reveal-line` (900ms unless the group sets it) and its steps are
 * `<RevealStep index={i} of={count}>`, timed from the same variable, so the
 * line and the steps cannot drift apart. Vertical by default; for a
 * horizontal line set `--reveal-line-from: 0 1` and `transformOrigin: left`.
 */
export function RevealLine(props: RevealProps) {
  return (
    <Box
      transformOrigin="top"
      _motionReduce={{ animation: 'none', scale: '1 1' }}
      selectors={{
        '[data-reveal-state="pending"] &': { scale: 'var(--reveal-line-from, 1 0)' },
        '[data-reveal-state="in"] &': {
          animationName: draw,
          animationDuration: 'var(--reveal-line, 900ms)',
          animationTimingFunction: 'linear',
          animationFillMode: 'both',
        },
      }}
      {...props}
    />
  );
}

/** A step on a `RevealLine`: appears as the line reaches it. */
export function RevealStep({ index, of, style, ...props }: RevealProps & { index: number; of: number }) {
  return (
    <Box
      style={{ ...style, '--reveal-index': index, '--reveal-count': of }}
      _motionReduce={{ animation: 'none' }}
      selectors={{
        '[data-reveal-state="pending"] &': { opacity: 0 },
        '[data-reveal-state="in"] &': {
          animationName: rise,
          animationDuration: '300ms',
          animationTimingFunction: 'ease-out',
          animationFillMode: 'both',
          animationDelay: 'calc(var(--reveal-line, 900ms) * var(--reveal-index) / var(--reveal-count))',
        },
      }}
      {...props}
    />
  );
}
