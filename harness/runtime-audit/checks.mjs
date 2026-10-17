// Geometry checks for a devup runtime audit plan. Pure functions over boxes
// measured in the page, so each rule is testable without a browser.
//
// A box is { x, y, width, height } in CSS px, all measured at the same moment.
// Every result is { status, reason, measured }: status is pass, fail or
// inconclusive, and inconclusive always says why nothing could be judged.

const DEFAULT_TOLERANCE = 0.5;

function right(box) {
  return box.x + box.width;
}

function bottom(box) {
  return box.y + box.height;
}

function round(value) {
  return Math.round(value * 100) / 100;
}

function rounded(box) {
  return box && { x: round(box.x), y: round(box.y), width: round(box.width), height: round(box.height) };
}

function missing(name, found) {
  if (found === 0) return `${name} matched no element`;
  if (found > 1) return `${name} matched ${found} elements; a check needs exactly one`;
  return null;
}

/**
 * @param {object} check  the plan's check entry
 * @param {object} measured  { subject, subjectCount, container, containerCount, page }
 */
export function evaluateCheck(check, measured) {
  const tolerance = check.tolerancePx ?? DEFAULT_TOLERANCE;
  if (check.kind === 'no-horizontal-overflow') {
    const { viewportWidth, scrollWidth, maxRight, overflowHidden } = measured.page;
    const widest = Math.max(scrollWidth, maxRight);
    const overflow = widest - viewportWidth;
    const evidence = { viewportWidth, scrollWidth: round(scrollWidth), maxRight: round(maxRight), overflowHidden };
    if (overflow > tolerance) {
      return {
        status: 'fail',
        reason: maxRight > scrollWidth + tolerance
          ? `content reaches ${round(maxRight)}px in a ${viewportWidth}px viewport, clipped by overflow so the page does not scroll`
          : `the page scrolls to ${round(scrollWidth)}px in a ${viewportWidth}px viewport`,
        measured: evidence,
      };
    }
    return { status: 'pass', reason: 'nothing reaches past the viewport', measured: evidence };
  }

  const subjectProblem = missing('subject', measured.subjectCount);
  if (subjectProblem) return { status: 'inconclusive', reason: subjectProblem, measured: {} };
  const subject = measured.subject;
  if (check.kind === 'visible') {
    const visible = subject.width > 0 && subject.height > 0 && measured.subjectVisible !== false;
    return {
      status: visible ? 'pass' : 'fail',
      reason: visible ? 'rendered with a non-empty box' : 'rendered with an empty or hidden box',
      measured: { subject: rounded(subject) },
    };
  }

  const containerProblem = missing('container', measured.containerCount);
  if (containerProblem) return { status: 'inconclusive', reason: containerProblem, measured: { subject: rounded(subject) } };
  const container = measured.container;
  const evidence = { subject: rounded(subject), container: rounded(container) };

  if (check.kind === 'contained') {
    const outside = [
      ['left', container.x - subject.x],
      ['top', container.y - subject.y],
      ['right', right(subject) - right(container)],
      ['bottom', bottom(subject) - bottom(container)],
    ].filter(([, by]) => by > tolerance);
    return outside.length === 0
      ? { status: 'pass', reason: 'inside its container on every edge', measured: evidence }
      : {
          status: 'fail',
          reason: `outside its container by ${outside.map(([edge, by]) => `${round(by)}px on the ${edge}`).join(', ')}`,
          measured: evidence,
        };
  }

  if (check.kind === 'aligned') {
    const expected = check.offsetPx ?? 0;
    const actual = check.edge === 'left'
      ? subject.x - container.x
      : check.edge === 'right'
        ? right(container) - right(subject)
        : (subject.x + subject.width / 2) - (container.x + container.width / 2);
    const off = Math.abs(actual - expected);
    const where = check.edge === 'center' ? 'from its container\'s centre' : `from its container's ${check.edge} edge`;
    return {
      status: off <= tolerance ? 'pass' : 'fail',
      reason: `${round(actual)}px ${where}, expected ${expected}px`,
      measured: { ...evidence, offset: round(actual) },
    };
  }

  if (check.kind === 'spans') {
    const inset = check.insetPx ?? 0;
    const leftGap = subject.x - container.x;
    const rightGap = right(container) - right(subject);
    const ok = Math.abs(leftGap - inset) <= tolerance && Math.abs(rightGap - inset) <= tolerance;
    return {
      status: ok ? 'pass' : 'fail',
      reason: `${round(leftGap)}px in from the left and ${round(rightGap)}px from the right, expected ${inset}px on both`,
      measured: { ...evidence, leftGap: round(leftGap), rightGap: round(rightGap) },
    };
  }

  return { status: 'inconclusive', reason: `unknown check kind ${check.kind}`, measured: {} };
}

/** Pure dimension comparison; raw CSS pixels are never multiplied by DPR. */
export function evaluateDimensions(check, measured, reference) {
  const tolerance = check.tolerancePx;
  if (!Number.isFinite(tolerance) || tolerance < 0 || tolerance > 10_000) {
    throw new Error('dimensions tolerancePx must be finite and in [0, 10000]');
  }
  const evidence = {
    units: 'css-px', subjectCount: measured.subjectCount,
    subjectVisible: measured.subjectVisible === true, subject: measured.subject ?? null,
  };
  const unknown = (reason) => ({ status: 'inconclusive', reason, measured: evidence });
  const problem = missing('subject', measured.subjectCount);
  if (problem) return unknown(problem);
  if (!reference || reference.status !== 'acquired') return unknown('no acquired Figma logical dimensions explicitly mapped at this width');
  if (![reference.width, reference.height, measured.subject?.width, measured.subject?.height]
    .every((value) => Number.isFinite(value) && value >= 0)) return unknown('logical dimensions or DOM CSS geometry is invalid');
  if (!evidence.subjectVisible) return unknown('the mapped element is hidden');
  const delta = { width: measured.subject.width - reference.width, height: measured.subject.height - reference.height };
  return {
    status: Math.abs(delta.width) <= tolerance && Math.abs(delta.height) <= tolerance ? 'pass' : 'fail',
    reason: 'DOM CSS pixels compared with acquired Figma logical dimensions',
    measured: evidence, deltaPx: delta, tolerancePx: tolerance,
  };
}


/** Text that only an error page shows. */
export const ERROR_PAGE_TEXT = [
  "This page couldn't load",
  'This page couldn’t load',
  'Application error',
  'Unhandled Runtime Error',
  'Internal Server Error',
  'Something went wrong',
];

/**
 * A page is healthy when it rendered without an error page, without uncaught
 * or console errors and without a failed navigation since it was opened.
 *
 * @param {object} observed  { errorTexts, errors }
 */
export function evaluatePageHealth(observed) {
  const problems = [
    ...observed.errorTexts.map((text) => `the page shows "${text}"`),
    ...observed.errors.map((error) => `${error.kind}: ${String(error.message).split('\n')[0]}`),
  ];
  return problems.length === 0
    ? { status: 'pass', reason: 'no error page, page error, console error or failed navigation', measured: {} }
    : { status: 'fail', reason: problems.slice(0, 5).join('; '), measured: { problems } };
}

/**
 * Layout from a Figma frame is converted statically, so at a drawn width it
 * is exact: either the converter wrote the right value or it did not. The
 * only allowance is rounding. Chromium reports getBoundingClientRect in
 * LayoutUnits (1/64 px) without snapping to device pixels at
 * deviceScaleFactor 1, while Figma keeps fractional geometry that the
 * converter writes as rounded px - so a correct element can sit up to half a
 * pixel from the frame, and nothing correct sits further.
 */
export const GEOMETRY_ROUNDING_PX = 0.5;

/**
 * Size and position of the subject relative to its container, against the
 * Figma frame's own geometry (expected: { x, y, width, height } in CSS px,
 * relative to the container's top-left). Exact up to GEOMETRY_ROUNDING_PX;
 * there is no tolerance to configure.
 */
export function evaluateBounds(check, measured) {
  const tolerance = GEOMETRY_ROUNDING_PX;
  const subjectProblem = missing('subject', measured.subjectCount);
  if (subjectProblem) return { status: 'inconclusive', reason: subjectProblem, measured: {} };
  const containerProblem = missing('container', measured.containerCount);
  if (containerProblem) return { status: 'inconclusive', reason: containerProblem, measured: {} };
  const { subject, container } = measured;
  const actual = {
    x: subject.x - container.x,
    y: subject.y - container.y,
    width: subject.width,
    height: subject.height,
  };
  const expected = check.expected;
  const off = (key) => actual[key] - expected[key];
  const position = ['x', 'y'].filter((key) => Math.abs(off(key)) > tolerance);
  const size = ['width', 'height'].filter((key) => Math.abs(off(key)) > tolerance);
  const exact = (box) => Object.fromEntries(Object.entries(box).map(([key, value]) => [key, Math.round(value * 1000) / 1000]));
  const evidence = { actual: exact(actual), expected, roundingPx: tolerance };
  if (position.length === 0 && size.length === 0) {
    return { status: 'pass', reason: 'size and position within tolerance', measured: evidence };
  }
  const parts = [
    ...position.map((key) => `${key} off by ${Math.round(off(key) * 100) / 100}px`),
    ...size.map((key) => `${key} off by ${Math.round(off(key) * 100) / 100}px`),
  ];
  return {
    status: 'fail',
    reason: `${position.length ? 'position' : 'size'}: ${parts.join(', ')}`,
    measured: { ...evidence, positionOff: position, sizeOff: size },
  };
}

/** An element a width does not draw: absent, or present with no visible box. */
export function evaluateHidden(observed) {
  if (observed.subjectCount === 0) return { status: 'pass', reason: 'not rendered at this width', measured: {} };
  if (observed.subjectCount > 1) return { status: 'inconclusive', reason: `subject matched ${observed.subjectCount} elements; a check needs exactly one`, measured: {} };
  return observed.visible
    ? { status: 'fail', reason: 'shown at a width its Figma frame does not draw it at', measured: {} }
    : { status: 'pass', reason: 'hidden at this width', measured: {} };
}

/** Text cut off by its own box: clipped overflow, not an intended ellipsis or line clamp. */
export function evaluateClippedText(observed) {
  return observed.clipped.length === 0
    ? { status: 'pass', reason: 'no text is cut off by its box', measured: {} }
    : {
        status: 'fail',
        reason: `text is cut off: ${observed.clipped.slice(0, 3).map((item) => `"${item.text}" in <${item.tag}>`).join(', ')}`,
        measured: { clipped: observed.clipped },
      };
}

/** Computed style properties a hover or focus state changes. */
export const STATE_PROPERTIES = [
  'color', 'backgroundColor', 'borderColor', 'boxShadow', 'outlineStyle', 'outlineWidth',
  'outlineColor', 'textDecorationLine', 'transform', 'opacity',
];

/**
 * A hover or focus-visible state must change something visible. Without a
 * subject the check swept every visible link and button: observed.items
 * holds each one, and any that changes nothing fails the check.
 */
export function evaluateInteraction(check, observed) {
  if (Array.isArray(observed.items)) {
    const changes = (item) => STATE_PROPERTIES.some((property) => item.before[property] !== item.after[property]);
    const flat = observed.items.filter((item) => !changes(item));
    if (observed.items.length === 0) return { status: 'pass', reason: 'no links or buttons to check', measured: { checked: 0 } };
    return flat.length === 0
      ? { status: 'pass', reason: `${check.state} changes all ${observed.items.length} links and buttons`, measured: { checked: observed.items.length } }
      : {
          status: 'fail',
          reason: `${check.state} changes nothing on ${flat.length} of ${observed.items.length}: ${flat.slice(0, 5).map((item) => `<${item.tag}> "${item.name}"`).join(', ')}`,
          measured: { checked: observed.items.length, unchanged: flat.map((item) => ({ tag: item.tag, name: item.name })) },
        };
  }
  const subjectProblem = missing('subject', observed.subjectCount);
  if (subjectProblem) return { status: 'inconclusive', reason: subjectProblem, measured: {} };
  const changed = STATE_PROPERTIES.filter((property) => observed.before[property] !== observed.after[property]);
  return changed.length > 0
    ? { status: 'pass', reason: `${check.state} changes ${changed.join(', ')}`, measured: { changed } }
    : { status: 'fail', reason: `${check.state} changes nothing visible`, measured: { before: observed.before } };
}

/** Counts recomputed from items, never taken from a declaration. */
/**
 * A view-transition check: following the subject link from one route to
 * another must run a view transition. View Transitions are a web standard, so
 * a navigation that runs none is a bug to fix, not a browser to blame.
 *
 * @param {object} observed  { subjectCount, supported, navigated, calls, from, to }
 *   calls counts document.startViewTransition calls (same-document, what
 *   React's <ViewTransition> uses) plus pagereveal events that carried a
 *   view transition (cross-document, the CSS @view-transition rule).
 */
export function evaluateViewTransition(observed) {
  const subjectProblem = missing('subject', observed.subjectCount);
  if (subjectProblem) return { status: 'inconclusive', reason: subjectProblem, measured: {} };
  const measured = { from: observed.from, to: observed.to, supported: observed.supported, calls: observed.calls };
  if (!observed.supported) {
    return { status: 'inconclusive', reason: 'this browser has no document.startViewTransition, so nothing can be judged', measured };
  }
  if (!observed.navigated) {
    return { status: 'inconclusive', reason: 'following the subject did not change the URL', measured };
  }
  if (observed.calls > 0) {
    return { status: 'pass', reason: `a view transition ran navigating from ${observed.from} to ${observed.to}`, measured };
  }
  return { status: 'fail', reason: `navigating from ${observed.from} to ${observed.to} ran no view transition`, measured };
}

export function summarize(runs) {
  const summary = { pass: 0, fail: 0, inconclusive: 0, skipped: 0 };
  for (const run of runs) {
    for (const screen of run.screens) {
      for (const item of screen.checks) {
        if (item.status in summary) summary[item.status] += 1;
      }
    }
  }
  return summary;
}
