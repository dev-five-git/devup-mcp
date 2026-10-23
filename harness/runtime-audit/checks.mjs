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
  if (Number.isFinite(observed.documentWidth) && observed.documentWidth < observed.viewportWidth - 1) {
    problems.push(`document is ${round(observed.documentWidth)}px wide in a ${observed.viewportWidth}px viewport`);
  }
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
  const childHint = (item) => item.childChanges?.length
    ? '; state styles are on a child (child computed style changes), not the focusable <a>/button; put states on the focusable element itself'
    : '';
  if (Array.isArray(observed.items)) {
    const changes = (item) => STATE_PROPERTIES.some((property) => item.before[property] !== item.after[property]);
    const flat = observed.items.filter((item) => !changes(item));
    if (observed.items.length === 0) return { status: 'pass', reason: 'no links or buttons to check', measured: { checked: 0 } };
    return flat.length === 0
      ? { status: 'pass', reason: `${check.state} changes all ${observed.items.length} links and buttons`, measured: { checked: observed.items.length } }
      : {
          status: 'fail',
          reason: `${check.state} changes nothing on ${flat.length} of ${observed.items.length}: ${flat.slice(0, 5).map((item) => `<${item.tag}> "${item.name}"${childHint(item)}`).join(', ')}`,
          measured: { checked: observed.items.length, unchanged: flat.map((item) => ({ tag: item.tag, name: item.name, childChanges: item.childChanges ?? [] })) },
        };
  }
  const subjectProblem = missing('subject', observed.subjectCount);
  if (subjectProblem) return { status: 'inconclusive', reason: subjectProblem, measured: {} };
  const changed = STATE_PROPERTIES.filter((property) => observed.before[property] !== observed.after[property]);
  return changed.length > 0
    ? { status: 'pass', reason: `${check.state} changes ${changed.join(', ')}`, measured: { changed } }
    : { status: 'fail', reason: `${check.state} changes nothing visible${childHint(observed)}`, measured: { before: observed.before, childChanges: observed.childChanges ?? [] } };
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

/** Parses a computed CSS colour (rgb/rgba, as getComputedStyle returns it). */
export function parseColor(text) {
  const match = /rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+%?))?\s*\)/.exec(String(text));
  if (!match) return null;
  let alpha = match[4] === undefined ? 1 : parseFloat(match[4]);
  if (String(match[4]).endsWith('%')) alpha /= 100;
  return { r: Number(match[1]), g: Number(match[2]), b: Number(match[3]), a: alpha };
}

function over(top, bottom) {
  const a = top.a + bottom.a * (1 - top.a);
  if (a === 0) return { r: 0, g: 0, b: 0, a: 0 };
  const mix = (key) => (top[key] * top.a + bottom[key] * bottom.a * (1 - top.a)) / a;
  return { r: mix('r'), g: mix('g'), b: mix('b'), a };
}

function luminance({ r, g, b }) {
  const channel = (value) => {
    const c = value / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** WCAG contrast ratio between two opaque colours. */
export function contrastRatio(first, second) {
  const [light, dark] = [luminance(first), luminance(second)].sort((a, b) => b - a);
  return (light + 0.05) / (dark + 0.05);
}

/**
 * The contrast of text against what is painted behind it: its own colour
 * over the stack of ancestor background colours (nearest first), on the
 * white canvas. Text over a background image cannot be judged from colours
 * and returns null.
 */
export function textContrast(sample) {
  if (sample.imageBehind) return null;
  const color = parseColor(sample.color);
  const canvas = parseColor(sample.canvasColor ?? 'rgb(255, 255, 255)');
  if (!color || !canvas) return null;
  if (sample.layers) {
    let ink = color;
    let backdrop = { r: 0, g: 0, b: 0, a: 0 };
    for (const layer of sample.layers) {
      const background = parseColor(layer.color);
      if (!background && (ink.a < 1 || backdrop.a < 1)) return null;
      if (background) {
        ink = over(ink, background);
        backdrop = over(backdrop, background);
      }
      ink.a *= layer.opacity;
      backdrop.a *= layer.opacity;
    }
    return contrastRatio(over(ink, canvas), over(backdrop, canvas));
  }
  let backdrop = canvas;
  for (const layer of [...sample.backgrounds].reverse()) {
    const parsed = parseColor(layer);
    if (!parsed) return null;
    backdrop = over(parsed, backdrop);
  }
  const ink = over({ ...color, a: color.a * (sample.opacity ?? 1) }, backdrop);
  return contrastRatio(ink, backdrop);
}

export const DEFAULT_MIN_HEIGHT_RATIO = 0.9;
export const DEFAULT_MIN_CONTRAST = 1.5;

/** Text uses computed colours, including browser default button backgrounds. */
export function evaluateNoLowContrastText(options, observed) {
  const minContrast = options.minContrast ?? DEFAULT_MIN_CONTRAST;
  const invisible = [];
  let judged = 0;
  let skipped = 0;
  for (const sample of observed.texts) {
    const contrast = textContrast(sample);
    if (contrast === null) { skipped += 1; continue; }
    judged += 1;
    if (contrast < minContrast) invisible.push({ text: sample.text, tag: sample.tag, contrast: round(contrast) });
  }
  return {
    status: invisible.length ? 'fail' : skipped ? 'inconclusive' : 'pass',
    reason: invisible.length
      ? `${invisible.length} texts cannot be seen against their background: ${invisible.slice(0, 3).map((item) => `"${item.text}" (${item.contrast}:1)`).join(', ')}`
      : skipped ? `${skipped} text samples have backgrounds or colours that cannot be judged` : `${judged} texts have visible contrast`,
    measured: { judged, skipped, invisible: invisible.slice(0, 20), minContrast },
  };
}

/** Every img and CSS background URL must decode to a nonzero natural width. */
export function evaluateImageLoaded(observed) {
  const resources = observed.resources ?? [];
  const broken = resources.filter((resource) => !resource.loaded);
  return {
    status: broken.length ? 'fail' : 'pass',
    reason: broken.length ? `${broken.length} images did not load: ${broken.slice(0, 5).map((item) => item.source).join(', ')}` : `${resources.length} image resources loaded`,
    measured: { listed: resources.length, broken },
  };
}

/**
 * Whether the page the browser rendered is the frame it was built from, not
 * merely code on disk: tall enough, every section and image present and
 * shown, images decoded, and no text a person cannot see.
 *
 * @param {object} completeness  the plan's { frameHeight, minHeightRatio, sections, images, minContrast }
 * @param {object} observed  { pageHeight, sections: [{ id, count, visible, width, height }],
 *   images: [{ id, count, visible, width, height, broken }], texts: [{ text, tag, color, backgrounds, imageBehind, opacity }] }
 */
export function evaluateRenderedCompleteness(completeness, observed) {
  const ratio = completeness.minHeightRatio ?? DEFAULT_MIN_HEIGHT_RATIO;
  const needed = completeness.frameHeight * ratio;
  const problems = [];
  if (!(observed.pageHeight >= needed)) {
    problems.push(`the page is ${round(observed.pageHeight)}px tall, under ${Math.round(ratio * 100)}% of the ${round(completeness.frameHeight)}px frame`);
  }
  const absent = (items) => items.filter((item) => item.count !== 1 || !item.visible || !(item.width > 0) || !(item.height > 0));
  const missingSections = absent(observed.sections);
  if (missingSections.length) {
    problems.push(`${missingSections.length} of ${observed.sections.length} sections not rendered: ${missingSections.slice(0, 5).map((item) => item.id).join(', ')}`);
  }
  const missingImages = absent(observed.images);
  if (missingImages.length) {
    problems.push(`${missingImages.length} of ${observed.images.length} images not rendered: ${missingImages.slice(0, 5).map((item) => item.id).join(', ')}`);
  }
  const broken = observed.images.filter((item) => !missingImages.includes(item) && item.broken);
  if (broken.length) {
    problems.push(`${broken.length} images did not load: ${broken.slice(0, 5).map((item) => item.id).join(', ')}`);
  }
  const contrast = evaluateNoLowContrastText(completeness, observed);
  const resources = evaluateImageLoaded(observed);
  if (contrast.status === 'fail') problems.push(contrast.reason);
  if (resources.status === 'fail') problems.push(resources.reason);
  const measured = {
    pageHeight: round(observed.pageHeight),
    frameHeight: completeness.frameHeight,
    minHeightRatio: ratio,
    sections: { listed: observed.sections.length, missing: missingSections.map((item) => item.id) },
    images: { listed: observed.images.length, missing: missingImages.map((item) => item.id), broken: broken.map((item) => item.id) },
    texts: contrast.measured,
    resources: resources.measured,
  };
  return problems.length === 0
    ? { status: contrast.status, reason: contrast.status === 'inconclusive' ? contrast.reason : `rendered complete: ${observed.sections.length} sections, ${observed.images.length} images, ${contrast.measured.judged} texts visible, ${round(observed.pageHeight)}px tall`, measured }
    : { status: 'fail', reason: problems.join('; '), measured };
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

/** Read independent numbered groups by visual rows, then left to right. */
export function evaluateSequenceOrder(observed) {
  const failures = [];
  let compared = 0;
  for (const group of observed.groups) {
    const numbers = group.map((item) => item.number).sort((a, b) => a - b);
    const consecutive = numbers.every((n, i) => i === 0 || n === numbers[i - 1] + 1);
    if (!consecutive) continue;
    const reading = [...group].sort((a, b) => a.y - b.y || a.x - b.x).map((item) => item.number);
    if (reading.length > 1 && reading.every((n, i) => i === 0 || n === reading[i - 1] - 1)) continue;
    compared += 1;
    const remaining = [...group];
    const ordered = [];
    while (remaining.length) {
      const top = Math.min(...remaining.map((item) => item.y));
      const row = remaining.filter((item) => item.y <= top + 8).sort((a, b) => a.x - b.x);
      ordered.push(...row);
      for (const item of row) remaining.splice(remaining.indexOf(item), 1);
    }
    for (let index = 1; index < ordered.length; index += 1) {
      if (ordered[index].number < ordered[index - 1].number) failures.push(`${ordered[index - 1].name} before ${ordered[index].name}`);
    }
  }
  if (failures.length) return { status: 'fail', reason: `sequence out of visual order: ${failures.join('; ')}`, measured: observed };
  // Numbered labels that formed no group were never compared: an empty
  // observation is not evidence of reading order.
  if (!compared && (observed.labels ?? 0) > 0) {
    return { status: 'pass', reason: `not applicable: ${observed.labels} numeric labels form no consecutive ascending sequence (statistics, descending lists, pagination)`, measured: observed };
  }
  if (!compared) return { status: 'pass', reason: 'no numbered labels on this page', measured: observed };
  return { status: 'pass', reason: `${compared} numbered sequences follow increasing visual order`, measured: observed };
}

/**
 * Whether the page's own styles arrived. A class token that no stylesheet rule
 * mentions means its CSS was never delivered (a cold dev server, a missing
 * per-file chunk); the page then renders as raw document flow.
 */
export function evaluateStylesApplied(observed) {
  const { classes = 0, unmatched = [], unreadableSheets = 0, styledElements = 0, elements = 0 } = observed;
  if (!elements) return { status: 'inconclusive', reason: 'the page has no elements to judge', measured: observed };
  if (unmatched.length) {
    const status = unreadableSheets ? 'inconclusive' : 'fail';
    return {
      status,
      reason: `${unmatched.length} of ${classes} class names have no CSS rule${unreadableSheets ? ` (${unreadableSheets} stylesheets could not be read)` : ''}: ${unmatched.slice(0, 8).join(', ')}`,
      measured: { ...observed, unmatched: unmatched.slice(0, 40) },
    };
  }
  if (classes > 0 && styledElements === 0) {
    return { status: 'fail', reason: 'no element received a style from its classes', measured: observed };
  }
  return { status: 'pass', reason: `${classes} class names all have CSS rules`, measured: observed };
}

export function evaluateSectionLayout(observed) {
  const failures = observed.sections.filter((item) => !item.visible || item.width < item.containerWidth * 0.5 || item.height <= 0);
  const narrow = observed.texts.filter((item) => item.width < 120);
  const problems = [...failures.map((item) => `${item.name}: section ${round(item.width)}px wide in ${round(item.containerWidth)}px container or hidden`), ...narrow.map((item) => `${item.name}: text column only ${round(item.width)}px wide`)];
  return { status: problems.length ? 'fail' : 'pass', reason: problems.length ? problems.join('; ') : 'sections visible and text columns have room', measured: observed };
}

export function evaluateTextAlignment(check, observed) {
  const actual = observed.align === 'start' ? (observed.direction === 'rtl' ? 'right' : 'left') : observed.align === 'end' ? (observed.direction === 'rtl' ? 'left' : 'right') : observed.align;
  const pass = observed.count === 1 && observed.visible && actual === check.edge;
  return { status: pass ? 'pass' : 'fail', reason: pass ? `text is ${actual} aligned as drawn` : `text alignment ${actual ?? 'missing'}, expected ${check.edge} (${observed.count} elements)`, measured: observed };
}
