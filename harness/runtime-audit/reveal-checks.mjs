// Reveal (entrance) checks. They observe rendered opacity, transform and
// position over time or over scroll, never a screenshot and never the
// presence of an animation declaration.
//
// Two kinds of entrance are measured, whatever produced them:
// - scroll-linked: CSS keyframes on a view timeline (`animation-timeline:
//   view()`). State is a function of scroll position, so there is no clock to
//   run late and nothing to replay; the check walks the block through its
//   entry and requires it fully shown once it is inside the viewport.
// - timed: keyframes, a transition or a script animation started once when
//   the block enters (a marker attribute, an observer). The check records
//   where the block was when the clock started, and that it never restarts.
//
// A result is `pass` only for what was observed. Anything that could not be
// observed is `unverified` (the check is then `inconclusive`, never `pass`),
// and a scenario with nothing to observe in this block is `not-applicable`.

/** Every scenario, in the order they run. Each starts from a fresh load. */
export const REVEAL_SCENARIOS = [
  'fast-scroll', 'reload', 'history', 'bfcache', 'client-nav', 'image-delay', 'image-failure',
  'focus', 'reduced-motion', 'no-script', 'stagger', 'progress-sync',
];

const FOCUSABLE = 'a[href],button,input,select,textarea,[tabindex]';

// Installed into the page; everything the checks read goes through it so the
// timed and scroll-linked paths agree on what "shown" and "in view" mean.
function installRevealLibrary() {
  if (window.__devupReveal) return;
  const frame = () => new Promise(resolve => requestAnimationFrame(resolve));
  const clocked = animation => !animation.timeline || animation.timeline instanceof DocumentTimeline;
  // The elements that carry the entrance: the named items of a group, or the
  // target itself.
  const parts = (node, items) => {
    const found = items ? [...node.querySelectorAll(items)] : [];
    return found.length ? found : [node];
  };
  // Opacity as painted, from a part up to the target.
  const paintedOpacity = (element, root) => {
    let value = 1;
    for (let current = element; current; current = current.parentElement) {
      value *= Number(getComputedStyle(current).opacity);
      if (current === root) break;
    }
    return value;
  };
  const shiftOf = element => new DOMMatrixReadOnly(getComputedStyle(element).transform).m42;
  // Where the block sits in layout: its own entrance translation must not
  // count as "not yet visible".
  const layoutBox = node => {
    const box = node.getBoundingClientRect(), shift = shiftOf(node);
    return { top: box.top - shift, bottom: box.bottom - shift, height: box.height };
  };
  const ratio = node => {
    const box = layoutBox(node);
    return Math.max(0, Math.min(box.bottom, innerHeight) - Math.max(box.top, 0)) / Math.min(box.height || 1, innerHeight);
  };
  // How far outside the viewport the layout box is. An observer reports a
  // block whose edge touches the viewport, so 1px is not "off screen".
  const offscreenPx = node => {
    const box = layoutBox(node);
    return Math.max(0, box.top - innerHeight, -box.bottom);
  };
  const sample = (node, items) => {
    const list = parts(node, items), box = node.getBoundingClientRect();
    const opacities = list.map(part => paintedOpacity(part, node)), shifts = list.map(part => Math.abs(shiftOf(part)));
    const animations = list.flatMap(part => part.getAnimations());
    return {
      x: box.x, y: box.y, width: box.width, height: box.height, scrollY,
      // The least and the most shown part: a group is mid-entrance while
      // some of its items are in and others are not.
      opacity: Math.min(...opacities), peakOpacity: Math.max(...opacities), translate: Math.max(...shifts),
      itemOpacities: list.length > 1 ? opacities : undefined,
      visibleRatio: ratio(node), offscreenPx: offscreenPx(node), visiblePx: ratio(node) * Math.min(box.height || 1, innerHeight),
      clockedAnimations: animations.filter(animation => clocked(animation) && animation.playState !== 'finished' && animation.playState !== 'idle').length,
      scrollLinked: animations.some(animation => !clocked(animation)),
    };
  };
  const shown = value => value.opacity === 1 && Math.abs(value.translate) < 0.1;
  const intermediate = value => (value.peakOpacity > 0 && value.opacity < 1) || (value.opacity === 1 && value.translate > 0.1);
  // A timed entrance has started once a clocked animation exists on a part,
  // or a part is part-way between hidden and shown.
  const started = value => value.clockedAnimations > 0 || (value.peakOpacity > 0 && value.opacity < 1);

  // Scrolls toward the target in small steps and records how much of it was
  // on screen when the entrance began and when it was complete.
  const approach = async (node, items) => {
    const first = sample(node, items);
    if (first.visibleRatio > 0) return { verified: false, reason: 'target is already in view' };
    const mode = first.scrollLinked ? 'scroll-linked' : 'timed';
    if (mode === 'timed' && started(first)) return { verified: true, mode, startedOffscreen: true, late: false, visibleRatio: 0, offscreenPx: first.offscreenPx, start: first };
    const step = Math.max(4, Math.round(innerHeight * (mode === 'timed' ? 0.05 : 0.02)));
    let fullSince = null, begun = null, middle = null;
    for (let index = 0; index < 6000; index++) {
      const top = node.getBoundingClientRect().top, position = scrollY;
      scrollBy(0, top > 0 ? step : -step); await frame(); await frame();
      const now = sample(node, items), stuck = scrollY === position;
      if (mode === 'timed') {
        if (started(now)) return { verified: true, mode, startedOffscreen: now.offscreenPx > 1, late: false, visibleRatio: now.visibleRatio, offscreenPx: now.offscreenPx, start: now };
        if (now.visibleRatio >= 0.99 || stuck) {
          fullSince ??= performance.now();
          if (performance.now() - fullSince > 400) return { verified: true, mode, startedOffscreen: false, late: true, visibleRatio: now.visibleRatio, start: now };
        }
        continue;
      }
      // Scroll-linked: progress is the state itself.
      if (!begun && (now.opacity > first.opacity || now.translate < first.translate - 0.1)) begun = now;
      if (!middle && intermediate(now) && now.opacity >= 0.15 && now.opacity <= 0.85) middle = now;
      if (shown(now)) return { verified: true, mode, startedOffscreen: (begun ?? now).offscreenPx > 1, late: false, visibleRatio: (begun ?? now).visibleRatio, offscreenPx: (begun ?? now).offscreenPx, completeRatio: now.visibleRatio, completeVisiblePx: now.visiblePx, start: first, middle: middle ?? (begun && intermediate(begun) ? begun : null), end: now };
      // Fully inside the viewport, or already half a screen of it showing
      // (or the page cannot scroll further), and still not shown: the range
      // runs past the entry, and a tall block would be read half faded.
      if (now.visibleRatio >= 0.99 || now.visiblePx >= Math.max(320, innerHeight * 0.5) || stuck) return { verified: true, mode, startedOffscreen: false, late: true, visibleRatio: (begun ?? now).visibleRatio, completeRatio: null, start: first, middle, end: now };
    }
    return { verified: true, mode, startedOffscreen: false, late: true, visibleRatio: ratio(node), start: first, middle, end: sample(node, items) };
  };

  // Watches the target for a while and reports whether it was ever hidden or
  // displaced while on screen.
  const watch = async (node, items, ms) => {
    const box = node.getBoundingClientRect();
    if (box.bottom <= 0 || box.top >= innerHeight) return { verified: false, reason: 'scroll position was not restored onto the target' };
    const end = performance.now() + ms; let minOpacity = 1, maxShift = 0;
    while (performance.now() < end) {
      const now = sample(node, items);
      // A block only partly on screen may still be entering by position.
      if (!(now.scrollLinked && now.visibleRatio < 0.99)) { minOpacity = Math.min(minOpacity, now.opacity); maxShift = Math.max(maxShift, now.translate); }
      await frame();
    }
    return { verified: true, minOpacity, maxShift, flashed: minOpacity < 1 || maxShift > 0.1 };
  };

  // When each item of a group started and finished appearing, in ms from the
  // moment the group was scrolled into view.
  const timeItems = async (node, items, ms) => {
    const list = [...node.querySelectorAll(items)];
    if (list.length < 2) return { verified: false, reason: 'fewer than two items matched' };
    node.scrollIntoView({ block: 'center' });
    const start = performance.now(), startedAt = list.map(() => null), shownAt = list.map(() => null), hiddenOnce = list.map(() => false);
    let scrollLinked = false;
    while (performance.now() - start < ms) {
      const now = performance.now() - start;
      list.forEach((item, index) => {
        const opacity = paintedOpacity(item, node), moved = Math.abs(shiftOf(item)) > 0.1;
        scrollLinked ||= item.getAnimations().some(animation => !clocked(animation));
        if (opacity < 0.01) { hiddenOnce[index] = true; startedAt[index] = null; shownAt[index] = null; return; }
        if (hiddenOnce[index] && startedAt[index] === null) startedAt[index] = now;
        if (opacity === 1 && !moved) { if (shownAt[index] === null) shownAt[index] = now; } else shownAt[index] = null;
      });
      if (shownAt.every(value => value !== null) && now > 100 && hiddenOnce.some(Boolean)) break;
      await frame();
    }
    return { verified: true, scrollLinked, observed: hiddenOnce.filter(Boolean).length, startedAt, shownAt };
  };

  window.__devupReveal = { frame, sample, shown, intermediate, approach, watch, timeItems };
}

const shown = value => value.opacity === 1 && Math.abs(value.translate) < 0.1;
const intermediate = value => (value.peakOpacity > 0 && value.opacity < 1) || (value.opacity === 1 && value.translate > 0.1);

async function library(page) {
  await page.evaluate(installRevealLibrary);
}

/**
 * Runs one reveal check. `context.capture(name)` is optional and saves a
 * frame screenshot; `context.take` receives every sample for the report.
 */
export async function runRevealCheck(page, options, measured, context = {}) {
  const ms = options.durationMs ?? 600, items = options.items ?? null, settle = options.settleMs ?? 400;
  const locate = tab => tab.locator(options.target);
  const target = locate(page);
  const sampleOn = async tab => { await library(tab); return locate(tab).evaluate((node, selector) => window.__devupReveal.sample(node, selector), items); };
  const take = async () => { const value = await sampleOn(page); measured.samples.push(value); return value; };
  const capture = async name => (context.capture ? context.capture(name).catch(() => null) : null);
  const frameAt = async (name, value) => ({ opacity: value.opacity, peakOpacity: value.peakOpacity, translate: value.translate, itemOpacities: value.itemOpacities, visibleRatio: value.visibleRatio, scrollY: value.scrollY, screenshot: await capture(name) ?? undefined });

  await library(page);
  // A descendant that animates while the target does not is the usual wrong
  // selector: the painted section was named instead of its inner wrapper.
  const carrier = await target.evaluate((node, selector) => {
    const own = selector ? [...node.querySelectorAll(selector)] : [node];
    const animated = new Set(node.getAnimations({ subtree: true }).map(animation => animation.effect?.target));
    return { own: own.some(part => animated.has(part)), descendant: animated.size > 0 };
  }, items);

  const trigger = await target.evaluate((node, selector) => window.__devupReveal.approach(node, selector), items);
  measured.trigger = trigger; measured.mode = trigger.mode ?? null;
  let valid = false, frames = null;
  if (trigger.mode === 'scroll-linked') {
    // Position decides the state, so each frame can be revisited exactly.
    frames = {};
    for (const [name, value] of [['start', trigger.start], ['middle', trigger.middle], ['end', trigger.end]]) {
      if (!value) continue;
      measured.samples.push(value);
      await page.evaluate(y => scrollTo(0, y), value.scrollY);
      await page.evaluate(() => window.__devupReveal.frame());
      frames[name] = await frameAt(name, value);
    }
    await page.evaluate(y => scrollTo(0, y), trigger.end.scrollY);
    valid = Boolean(trigger.middle) && intermediate(trigger.middle) && shown(trigger.end) && !trigger.late && !trigger.startedOffscreen;
  } else {
    if (!trigger.verified) await target.scrollIntoViewIfNeeded();
    const start = trigger.start ?? await take();
    if (trigger.start) measured.samples.push(start);
    frames = { start: await frameAt('start', start) };
    await page.waitForTimeout(Math.min(120, ms * 0.25));
    const middle = await take(); frames.middle = await frameAt('middle', middle);
    await page.waitForTimeout(ms + 30 + (items ? options.maxTotalMs ?? 1000 : 0));
    const last = await take(); frames.end = await frameAt('end', last);
    valid = intermediate(middle) && shown(last) && !(trigger.verified && (trigger.startedOffscreen || trigger.late));
  }
  measured.frames = frames;
  if (!valid && !carrier.own && carrier.descendant) measured.hint = 'The target itself does not animate but an element inside it does: point motion.target at the element that carries the entrance (the inner wrapper), or name the animated children with motion.items.';

  // A reveal plays once. Leave the viewport and come back: content must stay
  // shown while away (never re-hidden) and must not replay.
  const reference = await take();
  const leave = () => target.evaluate((node, above) => {
    const box = node.getBoundingClientRect(), top = scrollY + box.top, limit = document.documentElement.scrollHeight - innerHeight;
    // Scroll-linked: go past it (the block ends above the viewport), because
    // below the viewport it is hidden by position and nobody can see that.
    const candidates = above ? [Math.min(limit, top + box.height + 1)] : [0, limit];
    const destination = candidates.find(y => top + box.height <= y || top >= y + innerHeight);
    if (destination === undefined) return false;
    scrollTo(0, destination); return true;
  }, trigger.mode === 'scroll-linked');
  if (await leave()) {
    await page.waitForTimeout(60); const away = await take();
    await target.evaluate(node => node.scrollIntoView({ block: 'center' }));
    await page.waitForTimeout(trigger.mode === 'scroll-linked' ? 40 : Math.min(120, ms * 0.25)); const reentered = await take();
    await page.waitForTimeout(ms + 30); const settled = await take();
    measured.reentry = { verified: true, replayed: !shown(reentered), hiddenWhileAway: !shown(away) };
    if (trigger.mode === 'scroll-linked') {
      // From the top of the page straight to the block: the state must be
      // complete at once, with no clock to wait for.
      await page.evaluate(() => scrollTo(0, 0)); await page.waitForTimeout(40);
      await target.evaluate(node => node.scrollIntoView({ block: 'center' })); await page.waitForTimeout(40);
      const jumped = await take();
      measured.reentry.replayed ||= !shown(jumped);
    }
    valid = valid && shown(away) && shown(reentered) && shown(settled) && !measured.reentry.replayed;
  } else {
    // Nothing was observed, so nothing is claimed: this is not a pass.
    measured.reentry = { verified: false, reason: 'target cannot leave the viewport at this size' };
    if (valid) return { status: 'fail', reason: 'Reveal re-entry could not be observed: the target never leaves the viewport at this size. Check it at a viewport where it can scroll out.', measured };
  }

  // Scenarios. An explicit list is a claim that each one applies, so one with
  // nothing to observe fails. With no list every scenario runs, and one with
  // nothing to observe in this block is recorded as not-applicable.
  const explicit = Array.isArray(options.scenarios), requested = explicit ? options.scenarios : REVEAL_SCENARIOS;
  measured.scenarios = {}; measured.scenariosNotRun = REVEAL_SCENARIOS.filter(name => !requested.includes(name));
  const absent = (reason) => (explicit ? { status: 'fail', verified: false, reason } : { status: 'not-applicable', reason });
  const unverified = reason => ({ status: explicit ? 'fail' : 'unverified', verified: false, reason });
  const height = reference.height, url = page.url(), viewport = page.viewportSize();
  // A reload restores the scroll position, so start from the top: the target
  // must not already be on screen when the page loads.
  const fresh = async (waitUntil = 'load') => { await page.evaluate(() => scrollTo(0, 0)); await page.reload({ waitUntil }); await page.waitForTimeout(settle); await library(page); };
  const center = tab => locate(tab).evaluate(node => node.scrollIntoView({ block: 'center' }));
  const watchOn = async (tab, duration = ms + 600) => { await library(tab); return locate(tab).evaluate((node, config) => window.__devupReveal.watch(node, config.items, config.ms), { items, ms: duration }); };
  const verdict = observed => (observed.verified ? { status: observed.flashed ? 'fail' : 'pass', ...observed } : unverified(observed.reason));
  // A second context shares the session (cookies, storage) of the audited one.
  const sibling = async (browser, extra = {}) => browser.newContext({ viewport, storageState: await page.context().storageState(), ...extra });

  for (const name of requested) {
    let result;
    try {
      if (name === 'fast-scroll') {
        await fresh();
        await page.evaluate(async () => { for (let index = 0; index < 400 && scrollY < document.documentElement.scrollHeight - innerHeight - 1; index++) { scrollBy(0, innerHeight * 1.5); await new Promise(resolve => requestAnimationFrame(resolve)); } });
        await center(page); await page.waitForTimeout(ms + 150 + (items ? options.maxTotalMs ?? 1000 : 0));
        const value = await sampleOn(page); result = { status: shown(value) ? 'pass' : 'fail', sample: value };
      } else if (name === 'reload' || name === 'history') {
        await fresh(); await center(page); await page.waitForTimeout(ms + 150 + (items ? options.maxTotalMs ?? 1000 : 0));
        if (name === 'reload') await page.reload({ waitUntil: 'load' });
        else { await page.goto('about:blank'); await page.goBack({ waitUntil: 'load' }); if (page.url() !== url) throw new Error('back navigation did not return to the audited URL'); }
        result = verdict(await watchOn(page));
      } else if (name === 'bfcache') {
        result = await bfcache({ page, url, viewport, sibling, center, watchOn, ms, unverified, wait: ms + 150 + (items ? options.maxTotalMs ?? 1000 : 0), launch: context.launchRestorable });
      } else if (name === 'client-nav') {
        if (typeof options.navigate !== 'string') result = unverified('motion.navigate names no same-origin link to leave the page with, so client navigation was not exercised');
        else {
          await fresh(); await center(page); await page.waitForTimeout(ms + 150);
          await page.evaluate(() => { window.__devupSameDocument = true; });
          const link = page.locator(options.navigate);
          if (await link.count() !== 1) result = unverified('motion.navigate must match exactly one link');
          else {
            // Activated in place: scrolling to the link first would make the
            // router remember the link's position instead of the target's.
            await link.evaluate(node => node.click()); await page.waitForTimeout(settle);
            const left = page.url() !== url, sameDocument = await page.evaluate(() => window.__devupSameDocument === true);
            if (!left) result = unverified('following motion.navigate did not change the URL');
            else if (!sameDocument) result = { status: 'not-applicable', reason: 'the link loaded a new document: this app has no client navigation here (see the history scenario)' };
            else {
              await page.goBack({ waitUntil: 'commit' }); await page.waitForTimeout(60);
              if (page.url() !== url || await target.count() !== 1) result = unverified('going back did not return to the audited route');
              else result = { ...verdict(await watchOn(page)), sameDocument: true };
            }
          }
        }
      } else if (name === 'image-delay' || name === 'image-failure') {
        if (!await target.locator('img').count()) result = absent('target contains no image');
        else {
          const handler = async route => { if (route.request().resourceType() !== 'image') return route.continue(); if (name === 'image-failure') return route.abort(); await new Promise(resolve => setTimeout(resolve, ms + 1500)); return route.continue().catch(() => {}); };
          await page.route('**/*', handler);
          try { await fresh('domcontentloaded'); await center(page); await page.waitForTimeout(ms + 150 + (items ? options.maxTotalMs ?? 1000 : 0)); const value = await sampleOn(page); result = { status: shown(value) && Math.abs(value.height - height) < 1 ? 'pass' : 'fail', sample: value, reservedHeight: height }; }
          finally { await page.unroute('**/*', handler); }
        }
      } else if (name === 'focus') {
        await fresh();
        const focused = await target.evaluate(async (node, selector) => {
          const control = node.querySelector(selector); if (!control) return 'none';
          control.focus(); window.__devupFocused = control; for (let index = 0; index < 3; index++) await new Promise(resolve => requestAnimationFrame(resolve));
          return document.activeElement === control ? 'focused' : 'refused';
        }, options.focus ?? FOCUSABLE);
        if (focused !== 'focused') result = absent(focused === 'none' ? 'target has no focusable control' : 'the control inside the target did not take focus');
        else {
          await page.waitForTimeout(50); const value = await sampleOn(page);
          // The focused control is painted and on screen, and Tab can leave it.
          const control = await page.evaluate(() => { const element = document.activeElement, box = element.getBoundingClientRect(); let opacity = 1; for (let current = element; current; current = current.parentElement) opacity *= Number(getComputedStyle(current).opacity); return { opacity, onScreen: box.bottom > 0 && box.top < innerHeight }; });
          await page.keyboard.press('Tab');
          const moved = await page.evaluate(() => document.activeElement !== window.__devupFocused);
          result = { status: shown(value) && control.opacity === 1 && control.onScreen && moved ? 'pass' : 'fail', sample: value, focusedControl: control, trapped: !moved };
        }
      } else if (name === 'reduced-motion') {
        await page.emulateMedia({ reducedMotion: 'reduce' });
        try {
          await fresh();
          // Walk the block in and require it shown in place the whole way.
          const observed = await target.evaluate(async (node, selector) => {
            const reveal = window.__devupReveal; let minOpacity = 1, maxShift = 0, seen = 0;
            for (let index = 0; index < 6000; index++) {
              const top = node.getBoundingClientRect().top, position = scrollY;
              const value = reveal.sample(node, selector);
              if (value.visibleRatio > 0) { seen++; minOpacity = Math.min(minOpacity, value.opacity); maxShift = Math.max(maxShift, value.translate); }
              if (value.visibleRatio >= 0.99 && seen > 12) break;
              scrollBy(0, top > innerHeight / 2 ? Math.max(4, innerHeight * 0.05) : 0); await reveal.frame();
              if (scrollY === position && value.visibleRatio > 0 && seen > 12) break;
              if (scrollY === position && value.visibleRatio === 0) return { verified: false, reason: 'target never entered the viewport' };
            }
            return seen > 0 ? { verified: true, minOpacity, maxShift, flashed: minOpacity < 1 || maxShift > 0.1 } : { verified: false, reason: 'target never entered the viewport' };
          }, items);
          result = verdict(observed);
        } finally { await page.emulateMedia({ reducedMotion: null }); }
      } else if (name === 'no-script') {
        // Server markup is the content: with scripts off the block is there
        // and painted.
        const scriptless = await sibling(page.context().browser(), { javaScriptEnabled: false });
        try {
          const tab = await scriptless.newPage(); await tab.goto(url, { waitUntil: 'load' }); await tab.waitForTimeout(Math.min(settle, 300));
          if (await locate(tab).count() !== 1) result = unverified('target is not in the markup served without JavaScript');
          else { await center(tab); await tab.waitForTimeout(100); const value = await sampleOn(tab); result = { status: shown(value) || (value.scrollLinked && value.opacity === 1) ? 'pass' : 'fail', sample: value }; }
        } finally { await scriptless.close(); }
      } else if (name === 'stagger') {
        if (typeof items !== 'string') result = absent('motion.items names no group items');
        else {
          await fresh();
          const maxStep = options.maxStepMs ?? 150, maxSpan = options.maxSpanMs ?? 450, maxTotal = options.maxTotalMs ?? 1000;
          const observed = await target.evaluate((node, config) => window.__devupReveal.timeItems(node, config.items, config.ms), { items, ms: maxTotal + ms + 1500 });
          if (!observed.verified) result = absent(observed.reason);
          else if (observed.scrollLinked && observed.observed === 0) result = { status: 'not-applicable', reason: 'items are scroll-linked: their state follows position, there is no clock to bound' };
          else if (observed.observed === 0) result = unverified('no item was seen hidden before appearing, so no entrance was observed');
          else {
            const starts = observed.startedAt.filter(value => value !== null), ends = observed.shownAt;
            const steps = observed.startedAt.slice(1).map((value, index) => (value === null || observed.startedAt[index] === null ? null : value - observed.startedAt[index]));
            const spanMs = Math.max(...starts) - Math.min(...starts), finished = ends.every(value => value !== null);
            const totalMs = finished ? Math.max(...ends) - Math.min(...starts) : null;
            // A frame of jitter is not a reversed order.
            const ordered = steps.every(step => step === null || step >= -34);
            const ok = finished && ordered && steps.every(step => step === null || step <= maxStep) && spanMs <= maxSpan && totalMs <= maxTotal;
            result = { status: ok ? 'pass' : 'fail', items: observed.startedAt.length, startedAt: observed.startedAt, shownAt: observed.shownAt, stepsMs: steps, spanMs, totalMs, ordered, bounds: { maxStepMs: maxStep, maxSpanMs: maxSpan, maxTotalMs: maxTotal } };
          }
        }
      } else if (name === 'progress-sync') {
        // A progress line and the items it connects share one timeline: each
        // item appears as the line reaches it, within a tolerance.
        if (typeof options.progress !== 'string' || typeof items !== 'string') result = absent('progress-sync needs progress and items selectors');
        else {
          await fresh();
          const tolerance = options.syncToleranceMs ?? 200;
          const observed = await target.evaluate(async (node, config) => {
            const line = node.querySelector(config.progress), list = [...node.querySelectorAll(config.items)];
            if (!line || list.length < 2) return { verified: false, reason: 'progress line or at least two items not found' };
            node.scrollIntoView({ block: 'center' });
            const start = performance.now(), shownAt = list.map(() => null), reachedAt = list.map(() => null); let first = null, vertical = null;
            while (performance.now() - start < config.ms) {
              const box = line.getBoundingClientRect(), now = performance.now() - start; first ??= { width: box.width, height: box.height };
              if (vertical === null && (Math.abs(box.height - first.height) > 1 || Math.abs(box.width - first.width) > 1)) vertical = Math.abs(box.height - first.height) >= Math.abs(box.width - first.width);
              list.forEach((item, index) => {
                const rect = item.getBoundingClientRect();
                // The time that counts is the last rise above half opacity.
                let opacity = 1; for (let current = item; current; current = current.parentElement) { opacity *= Number(getComputedStyle(current).opacity); if (current === node) break; }
                if (opacity < 0.5) shownAt[index] = null; else if (shownAt[index] === null) shownAt[index] = now;
                if (reachedAt[index] === null && vertical !== null && (vertical ? box.bottom >= rect.top - 1 : box.right >= rect.left - 1)) reachedAt[index] = now;
              });
              await new Promise(resolve => requestAnimationFrame(resolve));
            }
            if (vertical === null) return { verified: false, reason: 'progress line never advanced' };
            return { verified: true, axis: vertical ? 'vertical' : 'horizontal', shownAt, reachedAt, gaps: list.map((_, index) => (shownAt[index] === null || reachedAt[index] === null ? null : shownAt[index] - reachedAt[index])) };
          }, { progress: options.progress, items, ms: options.progressMs ?? ms * 3 + 600 });
          result = observed.verified ? { status: observed.gaps.every(gap => gap !== null && Math.abs(gap) <= tolerance) ? 'pass' : 'fail', toleranceMs: tolerance, ...observed } : absent(observed.reason);
        }
      } else result = { status: 'fail', reason: 'unknown scenario' };
    } catch (error) { result = { status: explicit ? 'fail' : 'unverified', reason: String(error.message ?? error) }; }
    measured.scenarios[name] = result;
  }
  const named = status => Object.entries(measured.scenarios).filter(([, result]) => result.status === status).map(([name]) => name);
  measured.notApplicable = named('not-applicable');
  measured.unverified = [...named('unverified'), ...(trigger.verified ? [] : ['trigger'])];
  const failed = named('fail');
  if (failed.length) return { status: 'fail', reason: `Reveal scenario failed or could not be observed: ${failed.join(', ')}`, measured };
  if (!valid) return { status: 'fail', reason: 'Motion lacked intermediate geometry, continuity or expected final state', measured };
  if (measured.unverified.length) return { status: 'inconclusive', reason: `Reveal passed what was observed, but these could not be checked and are unverified: ${measured.unverified.join(', ')}`, measured };
  return { status: 'pass', reason: 'Observed start, intermediate and final frames, trigger position, one-shot re-entry and the requested scenarios', measured };
}

// Back/forward cache: the page is frozen and thawed, not reloaded. Playwright
// switches the cache off, so this needs a browser launched with it on.
async function bfcache({ page, url, viewport, sibling, center, watchOn, ms, wait, unverified, launch }) {
  let browser;
  try {
    browser = await (launch ?? (() => page.context().browser().browserType().launch({ channel: 'chromium', ignoreDefaultArgs: ['--disable-back-forward-cache'] })))();
  } catch (error) {
    return unverified(`no browser with the back/forward cache could be started (${String(error.message ?? error).split('\n')[0]})`);
  }
  try {
    const context = await sibling(browser);
    const tab = await context.newPage();
    await tab.addInitScript(() => addEventListener('pageshow', event => { window.__devupPageshow = [...(window.__devupPageshow ?? []), event.persisted]; }));
    await tab.goto(url, { waitUntil: 'load' }); await center(tab); await tab.waitForTimeout(wait);
    await tab.goto(new URL('/__devup-bfcache-away__', url).href, { waitUntil: 'commit' }).catch(() => {});
    await tab.goBack({ waitUntil: 'commit' }); await tab.waitForTimeout(30);
    const restored = tab.url() === url && await tab.evaluate(() => (window.__devupPageshow ?? []).includes(true)).catch(() => false);
    if (!restored) return unverified('the page was not restored from the back/forward cache (an unload handler, Cache-Control: no-store or an open connection makes it ineligible)');
    const observed = await watchOn(tab, ms + 600);
    return observed.verified ? { status: observed.flashed ? 'fail' : 'pass', restoredFromBfcache: true, ...observed } : unverified(observed.reason);
  } finally { await browser.close().catch(() => {}); }
}
