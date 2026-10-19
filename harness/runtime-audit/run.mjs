#!/usr/bin/env node
// Runs a devup runtime audit plan in the consumer project and writes the report
// devup_runtime_audit ingests.
//
//   node run.mjs --plan <plan.json> --out <report directory> [--max-minutes 10]
//
// The plan is data only: it is never evaluated. Each viewport gets a fresh
// browser context - no profile, cookies or storage from anyone's browser - and
// requests to origins the plan does not allow are blocked and recorded. The
// report always says what ran, what did not and why; a partial run is written
// as partial, never as complete.

import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { release } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { invalidateReloadedScreen, observeDevServer, serverLogReader } from './dev-environment.mjs';
import {
  ERROR_PAGE_TEXT, STATE_PROPERTIES, evaluateBounds, evaluateHidden, evaluateCheck, evaluateClippedText, evaluateDimensions,
  evaluateInteraction, evaluatePageHealth, evaluateTextAlignment, evaluateSequenceOrder, evaluateSectionLayout, evaluateNoLowContrastText, evaluateRenderedCompleteness, evaluateViewTransition, summarize,
} from './checks.mjs';
import {
  evaluateImagesLoaded, evaluateNativeAppearance, evaluateStaleResources, freshLoad, guardOrigins, observeImages, observeNativeAppearance, snapshotLoad,
} from './resource-checks.mjs';

const PLAN_KIND = 'devup-runtime-audit-plan';
const REPORT_KIND = 'devup-runtime-audit-report';
const DEFAULT_STEP_TIMEOUT = 10_000;

export function parseArgs(argv) {
  const args = { maxMinutes: 10 };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (flag === '--plan') args.plan = value;
    else if (flag === '--out') args.out = value;
    else if (flag === '--max-minutes') args.maxMinutes = Number(value);
    else if (flag === '--server-log') args.serverLog = value;
    else throw new Error(`unknown argument ${flag}`);
    index += 1;
  }
  if (!args.plan || !args.out || !(args.maxMinutes > 0)) {
    throw new Error('usage: node run.mjs --plan <plan.json> --out <dir> [--max-minutes N] [--server-log <file>]');
  }
  return args;
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function safeName(text) {
  return String(text).replace(/[^A-Za-z0-9._-]+/g, '-').slice(0, 80) || 'screen';
}

function locate(page, target) {
  if (target.role) return page.getByRole(target.role, target.name ? { name: target.name, exact: true } : {});
  if (target.testId) return page.getByTestId(target.testId);
  if (target.text) return page.getByText(target.text, { exact: true });
  return page.locator(target.css);
}

function classifyStepError(error) {
  const message = String(error?.message ?? error);
  if (/strict mode violation/i.test(message)) return { kind: 'ambiguous-locator', message };
  if (error?.name === 'TimeoutError' || /timeout/i.test(message)) return { kind: 'step-timeout', message };
  return { kind: 'step-error', message };
}

// The box a CSS mask actually paints inside its element: mask-size and
// mask-position resolved against the mask image's natural size, clipped to
// the element. A 1px rule drawn as `mask-size: contain` keeps its drawn
// length inside a wider element, and only this measurement sees that.
function paintedMaskBox(node) {
  const style = getComputedStyle(node);
  const rect = node.getBoundingClientRect();
  const image = style.maskImage || style.webkitMaskImage || '';
  const source = /url\(["']?(.*?)["']?\)/.exec(image)?.[1];
  if (!source) return Promise.resolve({ error: 'the element has no mask image' });
  const picture = new Image();
  picture.src = source;
  return picture.decode().then(
    () => {
      const naturalWidth = picture.naturalWidth;
      const naturalHeight = picture.naturalHeight;
      if (!naturalWidth || !naturalHeight) return { error: 'the mask image has no intrinsic size' };
      const size = (style.maskSize || style.webkitMaskSize || 'auto').trim();
      let width;
      let height;
      if (size === 'contain' || size === 'cover') {
        const pick = size === 'contain' ? Math.min : Math.max;
        const scale = pick(rect.width / naturalWidth, rect.height / naturalHeight);
        width = naturalWidth * scale;
        height = naturalHeight * scale;
      } else {
        const [first, second = 'auto'] = size.split(/\s+/);
        const length = (value, total, natural) => (value.endsWith('%')
          ? (total * parseFloat(value)) / 100
          : value.endsWith('px') ? parseFloat(value) : natural);
        width = length(first, rect.width, naturalWidth);
        height = second === 'auto' ? (width / naturalWidth) * naturalHeight : length(second, rect.height, naturalHeight);
      }
      const position = (style.maskPosition || style.webkitMaskPosition || '50% 50%').split(/\s+/);
      const offset = (value, free) => {
        if (value === 'center') return free / 2;
        if (value === 'left' || value === 'top') return 0;
        if (value === 'right' || value === 'bottom') return free;
        return value.endsWith('%') ? (free * parseFloat(value)) / 100 : parseFloat(value) || 0;
      };
      const left = Math.max(rect.left, rect.left + offset(position[0] ?? '50%', rect.width - width));
      const top = Math.max(rect.top, rect.top + offset(position[1] ?? position[0] ?? '50%', rect.height - height));
      const right = Math.min(rect.right, left + width);
      const bottom = Math.min(rect.bottom, top + height);
      return { box: { x: left, y: top, width: Math.max(0, right - left), height: Math.max(0, bottom - top) } };
    },
    () => ({ error: `the mask image ${source} did not load` }),
  );
}

async function measureBox(page, target, measure = 'box') {
  if (!target) return { count: 0 };
  const locator = locate(page, target);
  const count = await locator.count();
  if (count !== 1) return { count };
  const element = locator.first();
  const visible = await element.isVisible();
  if (measure === 'painted-mask') {
    const painted = await element.evaluate(paintedMaskBox);
    if (painted.error) return { count: 0, error: painted.error };
    return { count, box: painted.box, visible };
  }
  const box = await element.boundingBox();
  return { count, box: box ?? { x: 0, y: 0, width: 0, height: 0 }, visible };
}

// Counts the view transitions a navigation runs. A same-document one goes
// through document.startViewTransition, which React's <ViewTransition> calls;
// a cross-document one (the CSS @view-transition rule) arrives as a pagereveal
// event carrying a viewTransition. The count survives a document change in
// sessionStorage, which is per tab and origin.
const VIEW_TRANSITION_PROBE = () => {
  const key = '__devupViewTransitions';
  const bump = () => sessionStorage.setItem(key, String(Number(sessionStorage.getItem(key) || 0) + 1));
  window.addEventListener('pagereveal', (event) => {
    if (event.viewTransition) bump();
  });
  if (typeof document.startViewTransition === 'function' && !document.startViewTransition.__devup) {
    const original = document.startViewTransition;
    const counted = function startViewTransition(...args) {
      bump();
      return original.apply(this, args);
    };
    counted.__devup = true;
    document.startViewTransition = counted;
  }
};

async function observeViewTransition(page, check, timeout) {
  const target = locate(page, check.subject);
  const subjectCount = await target.count();
  if (subjectCount !== 1) return { subjectCount };
  const supported = await page.evaluate(() => typeof document.startViewTransition === 'function');
  const read = () => page.evaluate(() => Number(sessionStorage.getItem('__devupViewTransitions') || 0));
  const before = await read();
  const from = new URL(page.url()).pathname;
  await target.click({ timeout });
  const navigated = await page
    .waitForURL((url) => url.pathname !== from, { timeout })
    .then(() => true, () => false);
  let calls = 0;
  if (supported && navigated) {
    calls = await page
      .waitForFunction((start) => Number(sessionStorage.getItem('__devupViewTransitions') || 0) > start, before, { timeout: 2000 })
      .then(read, read)
      .then((now) => now - before);
  }
  return { subjectCount, supported, navigated, calls, from, to: new URL(page.url()).pathname };
}

// Text an element cuts off itself: overflow hidden or clipped past its box,
// not an intended ellipsis or line clamp.
async function clippedText(page, subject) {
  const scope = subject ? locate(page, subject) : page.locator('body');
  if ((await scope.count()) !== 1) return { clipped: [] };
  return scope.evaluate((root) => {
    const clipped = [];
    for (const element of [root, ...root.querySelectorAll('*')]) {
      if (clipped.length >= 10) break;
      const own = [...element.childNodes].filter((node) => node.nodeType === 3).map((node) => node.textContent.trim()).join(' ');
      if (!own) continue;
      const style = getComputedStyle(element);
      if (style.textOverflow === 'ellipsis' || (style.webkitLineClamp && style.webkitLineClamp !== 'none')) continue;
      const clips = (value) => value === 'hidden' || value === 'clip';
      const wide = clips(style.overflowX) && element.scrollWidth > element.clientWidth + 1;
      const tall = clips(style.overflowY) && element.scrollHeight > element.clientHeight + 1;
      if (wide || tall) clipped.push({ text: own.slice(0, 60), tag: element.tagName.toLowerCase() });
    }
    return { clipped };
  });
}

// Internal overflow can hide cards and navigation even when the page fits.
export function observeClippedContent(page) {
  return page.evaluate(() => {
    const clipped = [];
    for (const element of document.querySelectorAll('body *')) {
      if (!element.checkVisibility({ opacityProperty: true, visibilityProperty: true })) continue;
      const style = getComputedStyle(element);
      const box = element.getBoundingClientRect();
      if (!box.width || !box.height) continue;
      const horizontal = style.overflowX !== 'visible' && element.scrollWidth > element.clientWidth + 1;
      const vertical = ['hidden','clip'].includes(style.overflowY) && element.scrollHeight > element.clientHeight + 1;
      if (!horizontal && !vertical) continue;
      if (style.textOverflow === 'ellipsis' || (style.webkitLineClamp && style.webkitLineClamp !== 'none')) continue;
      const scrollbar = element.offsetHeight - element.clientHeight - parseFloat(style.borderTopWidth) - parseFloat(style.borderBottomWidth) > 2;
      const controls = element.parentElement?.querySelector('button[aria-label], [role="scrollbar"]');
      if (horizontal && !vertical && element.dataset.auditCarousel === 'true' && (scrollbar || controls)) continue;
      clipped.push({ name: element.id || `${element.tagName.toLowerCase()} "${element.textContent.trim().slice(0,60)}"`, horizontal, vertical });
    }
    return { clipped };
  });
}

// What the rendered page holds of its frame. A node is found by the id it
// carries itself, or, on a page merged from one frame per width, under this
// width in its data-figma-node-ids map.
export function observeCompleteness(page, completeness, width, subject) {
  const scope = subject ? locate(page, subject) : page.locator('body');
  return scope.evaluate(async (root, { sections, images, width: drawnWidth }) => {
    const find = (id) => document.querySelectorAll(
      `[data-figma-node-id="${id}"], [data-figma-node-ids~="${drawnWidth}:${id}"]`,
    );
    const shown = (element) => !element.checkVisibility
      || element.checkVisibility({ opacityProperty: true, visibilityProperty: true });
    const describe = (id) => {
      const found = find(id);
      if (found.length !== 1) return { id, count: found.length };
      const element = found[0];
      const rect = element.getBoundingClientRect();
      const img = element.tagName === 'IMG' ? element : null;
      return {
        id,
        count: 1,
        visible: shown(element),
        width: rect.width,
        height: rect.height,
        broken: img ? !(img.complete && img.naturalWidth > 0) : false,
      };
    };
    const canvasProbe = document.createElement('span');
    canvasProbe.style.backgroundColor = 'Canvas';
    document.body.appendChild(canvasProbe);
    const canvasColor = getComputedStyle(canvasProbe).backgroundColor;
    canvasProbe.remove();
    const texts = [];
    const walker = [...(root === document.body ? [document.documentElement] : []), root, ...root.querySelectorAll('*')];
    for (let index = 0; index < walker.length; index += 1) {
      const element = walker[index];
      const own = [...element.childNodes].filter((node) => node.nodeType === 3).map((node) => node.textContent.trim()).join(' ').trim();
      if (!own || !shown(element)) continue;
      const rect = element.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      const style = getComputedStyle(element);
      const backgrounds = [];
      const layers = [];
      let imageBehind = false;
      let backgroundExposed = true;
      let opacity = 1;
      for (let node = element; node && node.nodeType === 1; node = node.parentElement) {
        const nodeStyle = getComputedStyle(node);
        opacity *= Number(nodeStyle.opacity);
        layers.push({ color: nodeStyle.backgroundColor, opacity: Number(nodeStyle.opacity) });
        if (backgroundExposed) {
          if (nodeStyle.backgroundImage && nodeStyle.backgroundImage !== 'none') imageBehind = true;
          backgrounds.push(nodeStyle.backgroundColor);
          // An opaque colour covers images and colours on more distant ancestors.
          if (/^rgb\(/.test(nodeStyle.backgroundColor) || /,\s*1\)$/.test(nodeStyle.backgroundColor)) backgroundExposed = false;
        }
      }
      texts.push({ text: own.slice(0, 60), tag: element.tagName.toLowerCase(), color: style.color, canvasColor, backgrounds, layers, imageBehind, opacity });
    }
    const resources = [];
    const pending = [];
    const probe = (source, element, kind) => {
      const item = { source, tag: element.tagName.toLowerCase(), kind, loaded: false };
      resources.push(item);
      pending.push(new Promise((resolve) => {
        const image = new Image();
        const timer = setTimeout(() => resolve(), 5000);
        const done = () => { item.loaded = image.naturalWidth > 0; clearTimeout(timer); resolve(); };
        image.onload = done;
        image.onerror = done;
        image.src = source;
        if (image.complete) done();
      }));
    };
    for (const element of walker) {
      if (element.tagName === 'IMG') {
        const item = { source: element.currentSrc || element.src, tag: 'img', kind: 'img', loaded: false };
        resources.push(item);
        element.loading = 'eager';
        pending.push(new Promise((resolve) => {
          const timer = setTimeout(resolve, 5000);
          const done = () => { item.loaded = element.naturalWidth > 0; clearTimeout(timer); resolve(); };
          element.decode().then(done, done);
        }));
      }
      for (const pseudo of [null, '::before', '::after']) {
        const style = getComputedStyle(element, pseudo);
        if (pseudo && (style.content === 'none' || style.content === 'normal')) continue;
        for (const match of style.backgroundImage.matchAll(/url\(\s*(?:"([^"]*)"|'([^']*)'|([^)]*?))\s*\)/g)) {
          probe(match[1] ?? match[2] ?? match[3], element, `background${pseudo ?? ''}`);
        }
      }
    }
    await Promise.all(pending);
    return {
      resources,
      pageHeight: document.documentElement.scrollHeight,
      sections: sections.map(describe),
      images: images.map(describe),
      texts,
    };
  }, { sections: completeness.sections ?? [], images: completeness.images ?? [], width });
}

export function observeSequence(page) {
  return page.evaluate(() => {
    const groups = new Map();
    for (const element of document.querySelectorAll('body *')) {
      if (element.children.length || !element.checkVisibility({ opacityProperty: true, visibilityProperty: true })) continue;
      const label = element.textContent.trim();
      const match = /^(?:STEP\s*(\d{1,3})|(0\d|[1-9]\d?))[.?:]?$/i.exec(label);
      if (!match) continue;
      const number = Number(match[1] ?? match[2]);
      const scope = element.closest('section, [data-sequence-group], main') ?? document.body;
      const box = element.getBoundingClientRect();
      if (!box.width || !box.height) continue;
      const group = groups.get(scope) ?? [];
      group.push({ number, name: `${label} <${element.tagName.toLowerCase()}>`, x: box.x, y: box.y });
      groups.set(scope, group);
    }
    return { groups: [...groups.values()].filter((group) => group.length > 1 && new Set(group.map((item) => item.number)).size === group.length) };
  });
}

export async function observeSectionLayout(page, subject, container) {
  if (subject) {
    const target = locate(page, subject);
    const parent = locate(page, container);
    const box = await target.count() === 1 ? await target.boundingBox() : null;
    const containerBox = await parent.count() === 1 ? await parent.boundingBox() : null;
    return { sections: [{ name: JSON.stringify(subject), visible: Boolean(box) && await target.evaluate((element) => element.checkVisibility({ opacityProperty: true, visibilityProperty: true })), width: box?.width ?? 0, height: box?.height ?? 0, containerWidth: containerBox?.width ?? page.viewportSize().width }], texts: [] };
  }
  return page.evaluate(() => {
    const shown = (element) => element.checkVisibility({ opacityProperty: true, visibilityProperty: true });
    const sections = [...document.querySelectorAll('main > section, body > section')].map((element) => {
      const box = element.getBoundingClientRect();
      return { name: element.id || element.tagName, visible: shown(element), width: box.width, height: box.height, containerWidth: element.parentElement.getBoundingClientRect().width };
    });
    const texts = [...document.querySelectorAll('body *')].filter((element) => shown(element) && [...element.childNodes].filter((node) => node.nodeType === 3).map((node) => node.textContent.trim()).join(' ').length > 24).map((element) => ({ name: element.textContent.trim().slice(0, 60), width: element.getBoundingClientRect().width }));
    return { sections, texts };
  });
}

async function stateStyles(locator) {
  return locator.evaluate((element, properties) => {
    const style = getComputedStyle(element);
    return Object.fromEntries(properties.map((property) => [property, style[property]]));
  }, STATE_PROPERTIES);
}

// Hover moves the mouse over the subject; focus sets keyboard modality first
// so :focus-visible applies, as it does for a keyboard user. Both wait out
// the shared transition before reading the styles.
async function enterState(page, target, state, timeout) {
  const before = await stateStyles(target);
  if (state === 'focus') {
    await page.keyboard.press('Shift');
    await target.focus({ timeout });
  } else {
    await target.hover({ timeout });
  }
  await page.waitForTimeout(450);
  const after = await stateStyles(target);
  if (state === 'focus') await target.evaluate((element) => element.blur());
  await page.mouse.move(0, 0);
  await page.waitForTimeout(state === 'focus' ? 0 : 300);
  return { before, after };
}

async function observeInteraction(page, check, timeout) {
  if (!check.subject) {
    const all = page.locator('a[href], button, [role="button"]');
    const items = [];
    const count = Math.min(await all.count(), 40);
    for (let index = 0; index < count; index += 1) {
      const target = all.nth(index);
      if (!(await target.isVisible())) continue;
      const box = await target.boundingBox();
      if (!box || box.width === 0 || box.height === 0) continue;
      const described = await target.evaluate((element) => ({
        tag: element.tagName.toLowerCase(),
        name: (element.getAttribute('aria-label') || element.textContent || '').trim().slice(0, 40),
      }));
      const states = await enterState(page, target, check.state, timeout).catch(() => null);
      if (states) items.push({ ...described, ...states });
    }
    return { items };
  }
  const target = locate(page, check.subject);
  const subjectCount = await target.count();
  if (subjectCount !== 1) return { subjectCount };
  return { subjectCount, ...(await enterState(page, target, check.state, timeout)) };
}

async function pageMetrics(page) {
  return page.evaluate(() => {
    const viewportWidth = window.innerWidth;
    let maxRight = 0;
    let widest = null;
    const elements = document.body ? document.body.querySelectorAll('*') : [];
    for (let index = 0; index < elements.length && index < 20000; index += 1) {
      const element = elements[index];
      if (element.checkVisibility && !element.checkVisibility({ opacityProperty: true, visibilityProperty: true })) continue;
      const rect = element.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      // Content inside a horizontal scroll container (a carousel) scrolls
      // there; it is not page overflow. The container itself still counts.
      let scroller = element.parentElement;
      let inScroller = false;
      while (scroller && scroller !== document.body) {
        const overflowX = getComputedStyle(scroller).overflowX;
        if (overflowX === 'auto' || overflowX === 'scroll') {
          inScroller = true;
          break;
        }
        scroller = scroller.parentElement;
      }
      if (inScroller) continue;
      const right = rect.right + window.scrollX;
      if (right > maxRight) {
        maxRight = right;
        widest = `${element.tagName.toLowerCase()}${element.id ? `#${element.id}` : ''}`;
      }
    }
    const style = getComputedStyle(document.documentElement);
    const bodyStyle = document.body ? getComputedStyle(document.body) : style;
    return {
      viewportWidth,
      documentWidth: document.documentElement.getBoundingClientRect().width,
      scrollWidth: document.documentElement.scrollWidth,
      maxRight,
      widest,
      overflowHidden: [style.overflowX, bodyStyle.overflowX].some((value) => value === 'hidden' || value === 'clip'),
      fullHeight: Math.max(document.documentElement.scrollHeight, window.innerHeight),
    };
  });
}

async function waitReady(page, plan, timeout) {
  await page.evaluate(async () => {
    await document.fonts.ready;
    await Promise.all(Array.from(document.images).map((image) => image.decode().catch(() => undefined)));
  });
  if (plan.readiness?.selector) {
    await page.locator(plan.readiness.selector).first().waitFor({ state: 'visible', timeout: plan.readiness.timeoutMs ?? timeout });
  }
}

async function missingFonts(page, fonts) {
  if (!fonts?.length) return [];
  return page.evaluate((families) => families.filter((family) => !document.fonts.check(`16px "${family}"`)), fonts);
}

async function checkExpectations(page, expect, timeout) {
  if (!expect) return null;
  if (expect.urlPath) {
    await page.waitForURL((url) => url.pathname === expect.urlPath, { timeout });
  }
  if (expect.heading) {
    await page.getByRole('heading', { name: expect.heading, exact: true }).first().waitFor({ state: 'visible', timeout });
  }
  if (expect.visible) {
    await locate(page, expect.visible).first().waitFor({ state: 'visible', timeout });
  }
  return null;
}

async function runViewport(browser, plan, viewport, out, deadline, references, readServerLog) {
  const allowed = new Set(plan.allowedOrigins.map((origin) => new URL(origin).origin));
  const contextOptions = {
    viewport: { width: viewport.width, height: viewport.height },
    deviceScaleFactor: viewport.deviceScaleFactor ?? 1,
    isMobile: Boolean(viewport.mobile),
    hasTouch: Boolean(viewport.touch),
    locale: plan.locale ?? undefined,
    timezoneId: plan.timezone ?? undefined,
    serviceWorkers: 'block',
  };
  const context = await browser.newContext(contextOptions);
  const environmentErrors = [];
  const blocked = [];
  await guardOrigins(context, allowed, blocked);
  await context.addInitScript(VIEW_TRANSITION_PROBE);
  const page = await context.newPage();
  observeDevServer(page, environmentErrors);
  page.on('pageerror', (error) => environmentErrors.push({ kind: 'page-error', message: String(error?.message ?? error) }));
  page.on('console', (message) => {
    if (message.type() === 'error') environmentErrors.push({ kind: 'console-error', message: message.text() });
  });
  const responseLog = [];
  page.on('response', (response) => {
    const type = response.request().resourceType();
    if (['image', 'script', 'stylesheet', 'font', 'media'].includes(type)) {
      responseLog.push({ url: response.url(), status: response.status(), type });
    }
  });
  const failedLog = [];
  page.on('requestfailed', (request) => {
    const failure = request.failure()?.errorText ?? 'failed';
    if (failure.includes('ERR_BLOCKED_BY_CLIENT')) return;
    if (request.resourceType() === 'image') failedLog.push({ url: request.url(), failure });
    environmentErrors.push({ kind: 'request-failed', message: `${request.url()}: ${failure}` });
  });

  const run = {
    viewport: { width: viewport.width, height: viewport.height, deviceScaleFactor: viewport.deviceScaleFactor ?? 1 },
    startedAt: new Date().toISOString(),
    environmentErrors,
    screens: [],
  };
  const stepTimeout = plan.stepTimeoutMs ?? DEFAULT_STEP_TIMEOUT;
  let reached = true;
  try {
    for (const screen of plan.screens) {
      const record = { id: screen.id, status: 'completed', checks: [] };
      run.screens.push(record);
      const errorsAtStart = environmentErrors.length;
      const responsesAtStart = responseLog.length;
      const failedAtStart = failedLog.length;
      const notMeasured = (status, reason) => {
        record.checks = screen.checks.map((check, index) => ({ index, kind: check.kind, label: check.label, status, reason, measured: {} }));
      };
      if (Date.now() > deadline) {
        record.status = 'skipped';
        record.reason = 'the run reached its time limit';
        notMeasured('skipped', record.reason);
        continue;
      }
      if (!screen.routePath && !reached) {
        record.status = 'skipped';
        record.reason = 'the previous screen was not reached, so its steps cannot start';
        notMeasured('skipped', record.reason);
        continue;
      }
      try {
        try {
          if (screen.routePath) {
            const response = await page.goto(new URL(screen.routePath, plan.baseUrl).toString(), { waitUntil: 'load', timeout: stepTimeout });
            if (response && !response.ok()) {
              environmentErrors.push({ kind: 'navigation-failed', message: `${screen.routePath}: HTTP ${response.status()}` });
            }
          }
          for (const step of screen.steps ?? []) {
            const target = locate(page, step.target);
            const timeout = step.timeoutMs ?? stepTimeout;
            if (step.action === 'click') await target.click({ timeout });
            else await target.fill(step.value, { timeout });
          }
          await checkExpectations(page, screen.expect, stepTimeout);
          await waitReady(page, plan, stepTimeout);
        } catch (error) {
          const failure = classifyStepError(error);
          record.status = 'failed';
          record.reason = `${failure.kind}: ${failure.message.split('\n')[0]}`;
          reached = false;
          notMeasured('inconclusive', `the screen was not reached (${failure.kind})`);
          continue;
        }
        reached = true;
        record.url = page.url();
        const metrics = await pageMetrics(page);
        if (metrics.viewportWidth !== viewport.width) {
          environmentErrors.push({ kind: 'viewport-mismatch', message: `innerWidth ${metrics.viewportWidth} for a ${viewport.width}px viewport` });
        }
        for (const family of await missingFonts(page, plan.fonts)) {
          environmentErrors.push({ kind: 'font-missing', message: `${family} is not loaded` });
        }
        for (const [index, check] of screen.checks.entries()) {
          // A view-transition check navigates away, so it runs after every
          // other check and the screenshot have seen this screen.
          if (check.kind === 'view-transition') continue;
          if (Array.isArray(check.widths) && check.widths.length && !check.widths.includes(viewport.width)) continue;
          const item = { index, kind: check.kind, label: check.label };
          if (check.kind === 'page-health') {
            const text = await page.evaluate(() => (document.body ? document.body.innerText : ''));
            const errors = environmentErrors
              .slice(errorsAtStart)
              .filter((error) => ['page-error', 'console-error', 'navigation-failed'].includes(error.kind));
            record.checks.push({ ...item, ...evaluatePageHealth({ errorTexts: ERROR_PAGE_TEXT.filter((needle) => text.replaceAll("?", "'").includes(needle)), errors, documentWidth: metrics.documentWidth, viewportWidth: metrics.viewportWidth }) });
            continue;
          }
          if (check.kind === 'text-align') {
            const target = locate(page, check.subject);
            const count = await target.count();
            const observed = count === 1 ? await target.evaluate((element) => ({ count: 1, visible: element.checkVisibility({ opacityProperty: true, visibilityProperty: true }), align: getComputedStyle(element).textAlign, direction: getComputedStyle(element).direction })) : { count };
            record.checks.push({ ...item, ...evaluateTextAlignment(check, observed) });
            continue;
          }
          if (check.kind === 'no-clipped-content') {
            const observed = await observeClippedContent(page);
            record.checks.push({ ...item, status: observed.clipped.length ? 'fail' : 'pass', reason: observed.clipped.length ? `content cut off: ${observed.clipped.map((item) => item.name).join('; ')}` : 'no hidden internal content', measured: observed });
            continue;
          }
          if (check.kind === 'sequence-order') {
            record.checks.push({ ...item, ...evaluateSequenceOrder(await observeSequence(page)) });
            continue;
          }
          if (check.kind === 'section-layout') {
            record.checks.push({ ...item, ...evaluateSectionLayout(await observeSectionLayout(page, check.subject, check.container)) });
            continue;
          }
          if (check.kind === 'hidden') {
            const target = locate(page, check.subject);
            const subjectCount = await target.count();
            const visible = subjectCount === 1 && await target.evaluate((element) => {
              const rect = element.getBoundingClientRect();
              return rect.width > 0 && rect.height > 0
                && (!element.checkVisibility || element.checkVisibility({ opacityProperty: true, visibilityProperty: true }));
            });
            record.checks.push({ ...item, ...evaluateHidden({ subjectCount, visible }) });
            continue;
          }
          if (check.kind === 'image-loaded') {
            const scope = check.subject ? locate(page, check.subject) : page.locator('body');
            if ((await scope.count()) !== 1) {
              record.checks.push({ ...item, status: 'inconclusive', reason: 'subject must match exactly one element', measured: {} });
              continue;
            }
            const requests = {
              responses: responseLog.slice(responsesAtStart).filter((entry) => entry.type === 'image'),
              failed: failedLog.slice(failedAtStart),
              scoped: Boolean(check.subject),
            };
            record.checks.push({ ...item, ...evaluateImagesLoaded(await observeImages(scope), requests) });
            continue;
          }
          if (check.kind === 'no-low-contrast-text') {
            if (check.subject && await locate(page, check.subject).count() !== 1) {
              record.checks.push({ ...item, status: 'inconclusive', reason: 'subject must match exactly one element', measured: {} });
              continue;
            }
            const observed = await observeCompleteness(page, {}, viewport.width, check.subject);
            const result = evaluateNoLowContrastText({}, observed);
            record.checks.push({ ...item, ...result });
            continue;
          }
          if (check.kind === 'native-appearance') {
            const scope = check.subject ? locate(page, check.subject) : page.locator('body');
            if ((await scope.count()) !== 1) {
              record.checks.push({ ...item, status: 'inconclusive', reason: 'subject must match exactly one element', measured: {} });
              continue;
            }
            record.checks.push({ ...item, ...evaluateNativeAppearance(await observeNativeAppearance(scope)) });
            continue;
          }
          if (check.kind === 'stale-resources') {
            const responses = responseLog.slice(responsesAtStart);
            const first = await snapshotLoad(page);
            let second = null;
            let reloadError = null;
            try {
              second = await freshLoad({
                browser, contextOptions, url: page.url(), timeout: stepTimeout,
                install: (fresh) => guardOrigins(fresh, allowed, blocked),
                ready: (fresh) => waitReady(fresh, plan, stepTimeout),
              });
            } catch (error) {
              reloadError = String(error?.message ?? error).split('\n')[0];
            }
            record.checks.push({ ...item, ...evaluateStaleResources({ responses, first, second, reloadError }) });
            continue;
          }
          if (check.kind === 'rendered-completeness') {
            record.checks.push({ ...item, ...evaluateRenderedCompleteness(check.completeness, await observeCompleteness(page, check.completeness, viewport.width)) });
            continue;
          }
          if (check.kind === 'no-clipped-text') {
            record.checks.push({ ...item, ...evaluateClippedText(await clippedText(page, check.subject)) });
            continue;
          }
          if (check.kind === 'interaction') {
            const observed = await observeInteraction(page, check, stepTimeout).catch((error) => ({ subjectCount: 1, error: String(error?.message ?? error) }));
            record.checks.push({ ...item, ...(observed.error
              ? { status: 'inconclusive', reason: `the subject could not be ${check.state === 'focus' ? 'focused' : 'hovered'}: ${observed.error.split('\n')[0]}`, measured: {} }
              : evaluateInteraction(check, observed)) });
            continue;
          }
          if (check.kind === 'capture') {
            const target = locate(page, check.subject);
            const count = await target.count();
            if (count !== 1) {
              record.checks.push({ ...item, status: 'inconclusive', reason: count === 0 ? 'subject matched no element' : `subject matched ${count} elements; a check needs exactly one`, measured: {} });
              continue;
            }
            const shot = `${viewport.width}x${viewport.height}@${viewport.deviceScaleFactor ?? 1}-${safeName(screen.id)}-c${index}.png`;
            const bytes = await target.screenshot({ animations: 'disabled', caret: 'hide' });
            await writeFile(join(out, shot), bytes);
            record.checks.push({ ...item, status: 'pass', reason: 'captured for the server to compare with its Figma render', measured: {}, screenshot: { path: shot, sha256: sha256(bytes), bytes: bytes.length } });
            continue;
          }
          const subject = await measureBox(page, check.subject, check.measure);
          const container = await measureBox(page, check.container);
          const result = subject.error
            ? { status: 'inconclusive', reason: subject.error, measured: {} }
            : (check.kind === 'dimensions' ? evaluateDimensions : check.kind === 'bounds' ? evaluateBounds : evaluateCheck)(check, {
                page: metrics,
                subject: subject.box,
                subjectCount: subject.count,
                subjectVisible: subject.visible,
                container: container.box,
                containerCount: container.count,
              }, references.find((r) => r.screen === screen.id && r.check === index && r.viewportWidth === viewport.width));
          record.checks.push({ index, kind: check.kind, label: check.label, ...result });
        }
        const name = `${viewport.width}x${viewport.height}@${viewport.deviceScaleFactor ?? 1}-${safeName(screen.id)}.png`;
        const bytes = await page.screenshot({
          fullPage: true,
          clip: { x: 0, y: 0, width: metrics.viewportWidth, height: metrics.fullHeight },
          animations: 'disabled',
          caret: 'hide',
        });
        await writeFile(join(out, name), bytes);
        record.screenshot = { path: name, sha256: sha256(bytes), bytes: bytes.length };
        // Links on this page, so a dynamic route ([slug]) can be audited at the
        // URLs the site itself links to.
        record.links = await page.evaluate(() => {
          const seen = new Set();
          for (const anchor of document.querySelectorAll('a[href]')) {
            const url = new URL(anchor.getAttribute('href'), location.href);
            if (url.origin === location.origin) seen.add(url.pathname);
            if (seen.size >= 200) break;
          }
          return [...seen];
        }).catch(() => []);
        for (const [index, check] of screen.checks.entries()) {
          if (check.kind !== 'view-transition') continue;
          if (Array.isArray(check.widths) && check.widths.length && !check.widths.includes(viewport.width)) continue;
          const returnTo = page.url();
          const observed = await observeViewTransition(page, check, stepTimeout).catch((error) => ({
            subjectCount: 1, supported: true, navigated: false, calls: 0, error: String(error?.message ?? error),
          }));
          const result = observed.error
            ? { status: 'inconclusive', reason: `the subject could not be followed: ${observed.error.split('\n')[0]}`, measured: {} }
            : evaluateViewTransition(observed);
          record.checks.push({ index, kind: check.kind, label: check.label, ...result });
          // Back to where the check started, so a following screen reached by
          // steps starts from this screen whatever order the plan has.
          if (page.url() !== returnTo) {
            await page.goto(returnTo, { waitUntil: 'load', timeout: stepTimeout }).catch(() => {});
            await waitReady(page, plan, stepTimeout).catch(() => {});
          }
        }
        record.checks.sort((a, b) => a.index - b.index);
      } catch (error) {
        // A reload can destroy an execution context anywhere in measurement,
        // including screenshots. Unrelated executor errors still abort.
        environmentErrors.push(...await readServerLog());
        if (!invalidateReloadedScreen(record, environmentErrors.slice(errorsAtStart))) throw error;
        notMeasured('inconclusive', record.reason);
      } finally {
        environmentErrors.push(...await readServerLog());
        if (invalidateReloadedScreen(record, environmentErrors.slice(errorsAtStart))) reached = false;
      }
    }
  } finally {
    run.finishedAt = new Date().toISOString();
    if (blocked.length) {
      environmentErrors.push({ kind: 'request-blocked', message: `${blocked.length} request(s) outside allowedOrigins`, urls: blocked.slice(0, 20) });
    }
    await context.close();
  }
  return run;
}

export async function runPlan({ plan: planPath, out, maxMinutes, serverLog }) {
  const planBytes = await readFile(planPath);
  const document = JSON.parse(planBytes.toString('utf8'));
  if (document.kind !== PLAN_KIND || document.schemaVersion !== 1 || !document.plan) {
    throw new Error(`${planPath} is not a devup runtime audit plan; create one with devup_runtime_audit action plan`);
  }
  const plan = document.plan;
  const readServerLog = await serverLogReader(serverLog);
  validateDimensionsPlan(plan);
  if (!Number.isFinite(maxMinutes) || maxMinutes <= 0) throw new Error('maxMinutes must be finite and positive');
  const outDir = resolve(out);
  await mkdir(outDir, { recursive: true });
  const { chromium } = await import('playwright');
  const { version: playwrightVersion } = JSON.parse(
    await readFile(new URL('./node_modules/playwright/package.json', import.meta.url), 'utf8').catch(() => '{"version":null}'),
  );
  const report = {
    schemaVersion: 1,
    kind: REPORT_KIND,
    planId: document.planId,
    planSha256: sha256(planBytes),
    status: 'running',
    startedAt: new Date().toISOString(),
    executor: { name: 'devup-runtime-audit', node: process.version, playwright: playwrightVersion },
    environment: { os: `${process.platform} ${release()}`, locale: plan.locale ?? null, timezone: plan.timezone ?? null },
    runs: [],
  };
  const deadline = Date.now() + maxMinutes * 60_000;
  const writeReport = async () => {
    report.finishedAt = new Date().toISOString();
    report.summary = summarize(report.runs);
    await writeFile(join(outDir, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  };
  let aborted = false;
  const onSignal = () => {
    aborted = true;
  };
  process.once('SIGINT', onSignal);
  const browser = await chromium.launch();
  report.executor.browser = { name: 'chromium', version: browser.version() };
  try {
    for (const viewport of plan.viewports) {
      if (aborted || Date.now() > deadline) break;
      report.runs.push(await runViewport(browser, plan, viewport, outDir, deadline, document.dimensionReferences ?? [], readServerLog));
    }
    const ranEverything = report.runs.length === plan.viewports.length
      && report.runs.every((run) => run.screens.every((screen) => screen.status === 'completed'));
    report.status = aborted ? 'aborted' : ranEverything ? 'completed' : 'partial';
  } catch (error) {
    report.status = 'aborted';
    report.error = String(error?.message ?? error);
  } finally {
    process.off('SIGINT', onSignal);
    await browser.close();
    await writeReport();
  }
  return report;
}

export function validateDimensionsPlan(plan) {
  for (const screen of plan.screens) {
    for (const check of screen.checks) {
      if (check.tolerancePx != null && (!Number.isFinite(check.tolerancePx) || check.tolerancePx < 0 || check.tolerancePx > 10_000)) {
        throw new Error('tolerancePx must be finite and in [0, 10000]');
      }
      if (check.kind !== 'dimensions') continue;
      const mappings = check.figmaDimensions;
      if (!check.subject || check.container || !Number.isFinite(check.tolerancePx)
        || (check.measure != null && check.measure !== 'box') || !Array.isArray(mappings) || !mappings.length) {
        throw new Error('dimensions requires subject, tolerancePx, figmaDimensions and box measurement');
      }
      const widths = new Set();
      for (const reference of mappings) {
        if (widths.has(reference.viewportWidth) || !plan.viewports.some((v) => v.width === reference.viewportWidth)
          || typeof reference.nodeId !== 'string' || !reference.nodeId.trim()
          || typeof reference.artifactId !== 'string' || !reference.artifactId.trim()
          || Object.keys(reference).some((key) => !['viewportWidth', 'nodeId', 'artifactId'].includes(key))) {
          throw new Error('dimensions references require unique planned widths and explicit artifactId/nodeId only');
        }
        widths.add(reference.viewportWidth);
      }
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const report = await runPlan(parseArgs(process.argv.slice(2)));
    const { pass, fail, inconclusive, skipped } = report.summary;
    console.log(`${report.status}: ${pass} pass, ${fail} fail, ${inconclusive} inconclusive, ${skipped} skipped`);
  } catch (error) {
    console.error(String(error?.message ?? error));
    process.exitCode = 2;
  }
}
