// input-focus: a field's focus must mark the box the user sees.
//
// A text field is often an <input> with no border of its own inside a
// bordered container that also holds an icon, prefix or button. Focus lands
// on the <input>, so unless the container reacts to :focus-within, the only
// thing that shows is the browser's default outline inside the box - or
// nothing. Each visible input, textarea and select is focused twice, by mouse
// click and by keyboard Tab, and judged on what the browser actually paints:
// focus stays on the field, an indicator surrounds the visual container, no
// inner outline shows inside that container, nothing moves or resizes, and
// the box eases into its focus state (a CSS transition covers what changes)
// instead of snapping.

/** Text-entry and choice fields; buttons, checkboxes and the like are not fields. */
export const FIELD_SELECTOR = [
  'textarea', 'select',
  'input:not([type="hidden"]):not([type="checkbox"]):not([type="radio"]):not([type="range"]):not([type="color"])'
    + ':not([type="file"]):not([type="submit"]):not([type="button"]):not([type="reset"]):not([type="image"])',
].join(', ');

const MAX_FIELDS = 20;
const SETTLE_MS = 450;
/** Sub-pixel rounding is not movement. */
const SHIFT_PX = 0.5;
const PROBE = 'data-devup-focus-probe';

/** Paint that marks focus: border, ring, outline, background. */
const INDICATOR = [
  'borderTopColor', 'borderRightColor', 'borderBottomColor', 'borderLeftColor',
  'borderTopWidth', 'borderRightWidth', 'borderBottomWidth', 'borderLeftWidth',
  'boxShadow', 'outlineStyle', 'outlineWidth', 'outlineColor', 'backgroundColor',
];

const RECT = ['x', 'y', 'width', 'height'];

/**
 * Marks every visible field in scope and finds the box each one is drawn in.
 * The container is the field itself when it paints a border, background or
 * shadow; otherwise the nearest of three ancestors that does and is at most a
 * row taller than the field (an icon or button beside it, not a whole form).
 */
function markFields(root, { selector, probe, max }) {
  const transparent = (color) => !color || color === 'transparent' || /rgba\([^)]*,\s*0\)$/.test(color);
  const paints = (element) => {
    const style = getComputedStyle(element);
    const border = ['Top', 'Right', 'Bottom', 'Left'].some((side) => parseFloat(style[`border${side}Width`]) > 0
      && style[`border${side}Style`] !== 'none' && !transparent(style[`border${side}Color`]));
    return border || !transparent(style.backgroundColor) || style.boxShadow !== 'none';
  };
  const visible = (element) => {
    if (element.checkVisibility && !element.checkVisibility({ visibilityProperty: true, opacityProperty: true })) return false;
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  };
  for (const old of document.querySelectorAll(`[${probe}]`)) old.removeAttribute(probe);
  const fields = [];
  for (const field of [...(root.matches?.(selector) ? [root] : []), ...root.querySelectorAll(selector)]) {
    if (fields.length >= max || !visible(field)) continue;
    const rect = field.getBoundingClientRect();
    let container = field;
    if (!paints(field)) {
      let node = field.parentElement;
      for (let depth = 0; node && depth < 3 && node !== document.body; depth += 1, node = node.parentElement) {
        const box = node.getBoundingClientRect();
        if (box.height > Math.max(rect.height * 2, rect.height + 48)) break;
        if (paints(node)) { container = node; break; }
      }
    }
    const index = fields.length;
    field.setAttribute(probe, `field-${index}`);
    if (container !== field) container.setAttribute(probe, `box-${index}`);
    const label = field.labels?.[0]?.textContent?.trim()
      || field.getAttribute('aria-label')
      || (field.getAttribute('aria-labelledby') && document.getElementById(field.getAttribute('aria-labelledby'))?.textContent?.trim())
      || field.getAttribute('placeholder') || field.name || '';
    fields.push({
      index,
      tag: field.tagName.toLowerCase(),
      type: field.type ?? '',
      name: label.slice(0, 60),
      labelled: Boolean(label),
      disabled: field.disabled || field.matches(':disabled'),
      invalid: field.getAttribute('aria-invalid') === 'true' || field.matches(':invalid'),
      separateContainer: container !== field,
      containerTag: container.tagName.toLowerCase(),
      radius: getComputedStyle(container).borderRadius,
    });
  }
  return fields;
}

/** Computed paint and geometry of a field, its container and the next box in flow. */
function snapshotField({ probe, index, indicator, rect: keys }) {
  const field = document.querySelector(`[${probe}="field-${index}"]`);
  const container = document.querySelector(`[${probe}="box-${index}"]`) ?? field;
  const paint = (element) => {
    const style = getComputedStyle(element);
    return Object.fromEntries(indicator.map((property) => [property, style[property]]));
  };
  const rect = (element) => {
    if (!element) return null;
    const box = element.getBoundingClientRect();
    return Object.fromEntries(keys.map((key) => [key, box[key] + (key === 'y' ? scrollY : key === 'x' ? scrollX : 0)]));
  };
  let neighbour = container.nextElementSibling;
  while (neighbour && !neighbour.getClientRects().length) neighbour = neighbour.nextElementSibling;
  const active = document.activeElement;
  const motion = (element) => {
    const style = getComputedStyle(element);
    return { property: style.transitionProperty, duration: style.transitionDuration };
  };
  return {
    field: { paint: paint(field), rect: rect(field) },
    container: { paint: paint(container), rect: rect(container), motion: motion(container) },
    neighbour: rect(neighbour),
    focus: active === field ? 'field' : active === container && container !== field ? 'container'
      : active && active !== document.body ? (active.tagName.toLowerCase() + (active.id ? `#${active.id}` : '')) : 'none',
  };
}

/** Tabbable elements in document order, as Tab walks them (no positive tabindex reordering). */
function focusBefore({ probe, index }) {
  const field = document.querySelector(`[${probe}="field-${index}"]`);
  const tabbable = [...document.querySelectorAll('a[href], button, input, select, textarea, iframe, summary, [tabindex], [contenteditable="true"]')]
    .filter((element) => element.tabIndex >= 0 && !element.matches(':disabled') && element.getClientRects().length
      && !(element.tagName === 'INPUT' && element.type === 'hidden'));
  const at = tabbable.indexOf(field);
  document.activeElement?.blur?.();
  if (at > 0) {
    const previous = tabbable[at - 1];
    previous.focus({ preventScroll: true });
    // A tab stop that wraps the field is its container taking focus.
    if (previous.contains(field)) return 'container';
    return document.activeElement === previous ? 'previous' : 'none';
  }
  if (at !== 0) return 'unreachable';
  // The first tab stop: Tab from a blurred element continues after it, so
  // navigation restarts from a fixed, focusable-by-script marker at the top.
  const start = document.createElement('span');
  start.tabIndex = -1;
  start.setAttribute(probe, 'start');
  start.style.cssText = 'position:fixed;top:0;left:0;width:0;height:0;overflow:hidden';
  document.body.prepend(start);
  start.focus({ preventScroll: true });
  return 'start';
}

/**
 * Focuses each field by click and by Tab and records paint and geometry
 * before and after. Disabled fields are listed, never focused.
 *
 * @param {import('playwright').Page} page
 * @param {import('playwright').Locator} scope
 */
export async function observeInputFocus(page, scope, { timeout = 10_000 } = {}) {
  const fields = await scope.evaluate(markFields, { selector: FIELD_SELECTOR, probe: PROBE, max: MAX_FIELDS });
  const snap = (index) => page.evaluate(snapshotField, { probe: PROBE, index, indicator: INDICATOR, rect: RECT });
  const results = [];
  for (const field of fields) {
    if (field.disabled) {
      results.push({ ...field, modes: [] });
      continue;
    }
    const locator = page.locator(`[${PROBE}="field-${field.index}"]`);
    const modes = [];
    // Mouse: a click inside the field, where a user would put the caret.
    try {
      await page.evaluate(() => document.activeElement?.blur?.());
      await locator.scrollIntoViewIfNeeded({ timeout });
      await page.mouse.move(0, 0);
      // An eased state needs time to return to rest before it is measured.
      await page.waitForTimeout(SETTLE_MS);
      const before = await snap(field.index);
      await locator.click({ timeout, position: { x: 4, y: 4 }, noWaitAfter: true });
      // The pointer leaves so a hover style is not mistaken for focus.
      await page.mouse.move(0, 0);
      await page.waitForTimeout(SETTLE_MS);
      const after = await snap(field.index);
      if (field.tag === 'select') await page.keyboard.press('Escape');
      modes.push({ mode: 'click', before, after });
    } catch (error) {
      modes.push({ mode: 'click', error: String(error?.message ?? error).split('\n')[0] });
    }
    // Keyboard: focus the tabbable element before the field, then Tab.
    try {
      await page.mouse.move(0, 0);
      const start = await page.evaluate(focusBefore, { probe: PROBE, index: field.index });
      if (start === 'unreachable') {
        modes.push({ mode: 'tab', unreachable: true });
      } else {
        await page.waitForTimeout(SETTLE_MS);
        const before = await snap(field.index);
        await page.keyboard.press('Tab');
        await page.waitForTimeout(SETTLE_MS);
        const after = await snap(field.index);
        await page.evaluate((probe) => document.querySelector(`[${probe}="start"]`)?.remove(), PROBE);
        modes.push({ mode: 'tab', before, after, ...(start === 'container' ? { containerTabStop: true } : {}) });
      }
    } catch (error) {
      modes.push({ mode: 'tab', error: String(error?.message ?? error).split('\n')[0] });
    }
    await page.evaluate(() => document.activeElement?.blur?.());
    results.push({ ...field, modes });
  }
  await page.evaluate((probe) => { for (const element of document.querySelectorAll(`[${probe}]`)) element.removeAttribute(probe); }, PROBE)
    .catch(() => {});
  return { fields: results };
}

const visibleOutline = (paint) => paint.outlineStyle !== 'none' && parseFloat(paint.outlineWidth) > 0;
const changed = (before, after) => INDICATOR.filter((property) => before[property] !== after[property]);
const moved = (before, after) => Boolean(before && after) && RECT.some((key) => Math.abs(before[key] - after[key]) > SHIFT_PX);

/** `borderTopColor` -> `border-top-color`. */
const kebab = (property) => property.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`);

/**
 * The changed paint properties no CSS transition eases: a property is eased
 * when `transition-property` names it, its shorthand (`border-color`,
 * `border`, `outline`) or `all`, with a duration above zero. `outline-style`
 * is discrete and never needs one. Missing motion data (an older snapshot)
 * eases everything, so it never fails on its own.
 */
export function snappedProperties(changedProperties, motion) {
  if (!motion) return [];
  const names = String(motion.property ?? '').split(',').map((name) => name.trim());
  const durations = String(motion.duration ?? '').split(',').map((value) => parseFloat(value) || 0);
  const duration = (at) => durations[at % Math.max(durations.length, 1)] ?? 0;
  return changedProperties.filter((property) => property !== 'outlineStyle').filter((property) => {
    const css = kebab(property);
    const sideless = css.replace(/-(top|right|bottom|left)(?=-|$)/, '');
    return !names.some((name, at) => duration(at) > 0 && (name === 'all' || name === css || name === sideless
      || css.startsWith(`${name}-`) || sideless.startsWith(`${name}-`)));
  });
}

/** Problems with one focus of one field; empty when it passes. */
export function judgeFocus(field, mode) {
  const { before, after } = mode;
  if (after.focus === 'container') {
    return ['focus moved to the container instead of the field'];
  }
  if (after.focus !== 'field') {
    return [`focus did not reach the field (active: ${after.focus})`];
  }
  const problems = mode.containerTabStop ? ['Tab stops on a container wrapping the field before the field itself'] : [];
  const boxChange = changed(before.container.paint, after.container.paint);
  const innerOutline = field.separateContainer && visibleOutline(after.field.paint)
    && (changed(before.field.paint, after.field.paint).length > 0);
  const innerRing = field.separateContainer && before.field.paint.boxShadow !== after.field.paint.boxShadow;
  if (!field.separateContainer) {
    if (boxChange.length === 0) problems.push('no focus indicator: nothing on the field changes');
  } else if (boxChange.length === 0) {
    problems.push(innerOutline || innerRing
      ? `focus marks only the inner ${field.tag}, not the <${field.containerTag}> box around it`
      : `no focus indicator: neither the ${field.tag} nor its <${field.containerTag}> box changes`);
  } else if (innerOutline || innerRing) {
    problems.push(`a second ${innerOutline ? 'outline' : 'ring'} shows on the inner ${field.tag} inside the <${field.containerTag}> box's focus style`);
  }
  const snapped = boxChange.length ? snappedProperties(boxChange, after.container.motion) : [];
  if (snapped.length) {
    problems.push(`the focus state snaps with no transition on ${[...new Set(snapped.map(kebab).map((name) => name.replace(/-(top|right|bottom|left)(?=-|$)/, '')))].join(', ')}`);
  }
  if (moved(before.container.rect, after.container.rect) || moved(before.field.rect, after.field.rect)) {
    problems.push('the field or its box changes size or position when focused');
  } else if (moved(before.neighbour, after.neighbour)) {
    problems.push('the element after the field moves when it is focused');
  }
  return problems;
}

/** Fails any field whose focus, by click or by Tab, does not mark its box cleanly. */
export function evaluateInputFocus(observed) {
  const fields = observed.fields ?? [];
  const enabled = fields.filter((field) => !field.disabled);
  if (fields.length === 0) return { status: 'pass', reason: 'no input, textarea or select to check', measured: { checked: 0, fields: [] } };
  const rows = [];
  const failures = [];
  const unjudged = [];
  for (const field of enabled) {
    for (const mode of field.modes) {
      const where = `<${field.tag}> "${field.name || field.index}" by ${mode.mode}`;
      if (mode.error || mode.unreachable) {
        unjudged.push(`${where}: ${mode.error ?? 'not reachable with Tab'}`);
        rows.push({ index: field.index, mode: mode.mode, status: 'inconclusive', reason: mode.error ?? 'not reachable with Tab' });
        continue;
      }
      const problems = judgeFocus(field, mode);
      if (problems.length) failures.push(`${where}: ${problems.join(', ')}`);
      rows.push({
        index: field.index, mode: mode.mode, status: problems.length ? 'fail' : 'pass', problems,
        changed: changed(mode.before.container.paint, mode.after.container.paint),
      });
    }
  }
  const measured = {
    checked: enabled.length,
    disabled: fields.length - enabled.length,
    fields: fields.map(({ modes, ...field }) => ({
      ...field,
      modes: rows.filter((row) => row.index === field.index).map(({ index, ...row }) => row),
    })),
  };
  if (failures.length) {
    return { status: 'fail', reason: `${failures.length} focus problem(s): ${failures.slice(0, 4).join('; ')}`, measured };
  }
  if (unjudged.length) {
    return { status: 'inconclusive', reason: `could not focus: ${unjudged.slice(0, 4).join('; ')}`, measured };
  }
  return {
    status: 'pass',
    reason: `focus by click and Tab marks the box of all ${enabled.length} field(s), eased, with no inner outline or movement`
      + (measured.disabled ? `; ${measured.disabled} disabled field(s) not focused` : ''),
    measured,
  };
}
