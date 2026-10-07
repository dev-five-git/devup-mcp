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

/** Counts recomputed from items, never taken from a declaration. */
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
