// The executor against a real Chromium: the hand-widened screen passes at the
// width it was drawn at and fails each named relation at every other width,
// and the policy-anchored screen keeps every relation at all four widths.

import assert from 'node:assert/strict';
import { readFile, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';
import test from 'node:test';
import { observeClippedContent, observeCompleteness, runPlan } from '../run.mjs';
import { observeNativeAppearance } from '../resource-checks.mjs';

const FIXTURES = new URL('../fixtures/', import.meta.url);
const TYPES = { '.html': 'text/html; charset=utf-8', '.svg': 'image/svg+xml' };

test('measurement probes tolerate a document without a body', async () => {
  const { chromium } = await import('playwright');
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.setContent('<body><button>Test</button></body>');
    await page.evaluate(() => document.body.remove());
    assert.deepEqual(await observeClippedContent(page), { clipped: [] });
    await observeCompleteness(page, {}, 800, { css: 'html' });
    await observeNativeAppearance(page.locator('html'));
  } finally {
    await browser.close();
  }
});

test('a measurement exception is inconclusive and later checks still run', async () => {
  const server = await serve();
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const out = await mkdtemp(join(tmpdir(), 'devup-check-error-'));
  try {
    const plan = join(out, 'plan.json');
    await writeFile(plan, JSON.stringify({ schemaVersion: 1, kind: 'devup-runtime-audit-plan', planId: 'check-error', plan: {
      baseUrl, allowedOrigins: [baseUrl], viewports: [{ width: 800, height: 600 }],
      screens: [{ id: 'home', routePath: '/', checks: [
        { kind: 'text-align', subject: { css: '[' }, edge: 'left' },
        { kind: 'page-health' },
      ] }],
    } }));
    const report = await runPlan({ plan, out, maxMinutes: 1 });
    assert.equal(report.status, 'completed');
    assert.deepEqual(report.runs[0].screens[0].checks.map((check) => check.status), ['inconclusive', 'pass']);
    assert.match(report.runs[0].screens[0].checks[0].reason, /home\/800\/check 0/);
  } finally {
    server.close();
    await rm(out, { recursive: true, force: true });
  }
});

function serve() {
  const server = createServer(async (request, response) => {
    const name = new URL(request.url, 'http://local').pathname.slice(1) || 'before.html';
    if (!/^[a-z]+\.(html|svg)$/.test(name)) {
      response.writeHead(404).end();
      return;
    }
    try {
      const body = await readFile(new URL(name, FIXTURES));
      response.writeHead(200, { 'content-type': TYPES[extname(name)] }).end(body);
    } catch {
      response.writeHead(404).end();
    }
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}

const CHECKS = [
  { kind: 'no-horizontal-overflow', label: 'page' },
  { kind: 'contained', label: 'badge text', subject: { testId: 'badge-text' }, container: { testId: 'badge' } },
  { kind: 'aligned', label: 'qr', edge: 'right', offsetPx: 16, subject: { testId: 'qr' }, container: { testId: 'food-card' } },
  { kind: 'spans', label: 'rule', insetPx: 15.5, measure: 'painted-mask', subject: { testId: 'divider' }, container: { testId: 'card' } },
  { kind: 'aligned', label: 'notice', edge: 'center', subject: { testId: 'notice' }, container: { testId: 'screen' } },
];

test('the widened screen fails off its drawn width and the anchored one holds everywhere', async () => {
  const server = await serve();
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const out = await mkdtemp(join(tmpdir(), 'devup-runtime-audit-'));
  try {
    const planPath = join(out, 'plan.json');
    await writeFile(planPath, JSON.stringify({
      schemaVersion: 1,
      kind: 'devup-runtime-audit-plan',
      planId: 'browser-test',
      plan: {
        baseUrl,
        allowedOrigins: [baseUrl],
        viewports: [360, 768, 1200, 1440].map((width) => ({ width, height: 800, deviceScaleFactor: 1 })),
        screens: [
          { id: 'before', routePath: '/before.html', checks: CHECKS },
          { id: 'after', routePath: '/after.html', checks: CHECKS },
        ],
        // The server writes every option it was not given as null.
        locale: null,
        timezone: null,
        readiness: null,
        stepTimeoutMs: null,
      },
    }));
    const report = await runPlan({ plan: planPath, out, maxMinutes: 5 });
    assert.equal(report.status, 'completed', JSON.stringify(report, null, 2));
    assert.equal(report.runs.length, 4);
    for (const run of report.runs) {
      assert.deepEqual(run.environmentErrors, [], `${run.viewport.width}: ${JSON.stringify(run.environmentErrors)}`);
      const [before, after] = run.screens;
      const statuses = (screen) => Object.fromEntries(screen.checks.map((item) => [item.label, item.status]));
      assert.deepEqual(
        Object.values(statuses(after)),
        ['pass', 'pass', 'pass', 'pass', 'pass'],
        `after at ${run.viewport.width}: ${JSON.stringify(after.checks, null, 2)}`,
      );
      const expected = run.viewport.width === 360
        ? { page: 'pass', 'badge text': 'pass', qr: 'pass', rule: 'pass', notice: 'pass' }
        : { page: 'pass', 'badge text': 'fail', qr: 'fail', rule: 'fail', notice: 'fail' };
      assert.deepEqual(statuses(before), expected, `before at ${run.viewport.width}: ${JSON.stringify(before.checks, null, 2)}`);
      for (const screen of run.screens) {
        assert.match(screen.screenshot.sha256, /^[0-9a-f]{64}$/);
        const bytes = await readFile(join(out, screen.screenshot.path));
        assert.equal(bytes.length, screen.screenshot.bytes);
        assert.equal(bytes.readUInt32BE(16), run.viewport.width, 'the capture is the viewport width');
      }
    }
    assert.deepEqual(report.summary, { pass: 28, fail: 12, inconclusive: 0, skipped: 0 });
    const written = JSON.parse(await readFile(join(out, 'report.json'), 'utf8'));
    assert.equal(written.planSha256.length, 64);
    assert.equal(written.kind, 'devup-runtime-audit-report');
  } finally {
    server.close();
    await rm(out, { recursive: true, force: true });
  }
});

test('steps reach a screen by role and name, and a failed step leaves the screens after it unreached', async () => {
  const server = await serve();
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const out = await mkdtemp(join(tmpdir(), 'devup-runtime-audit-'));
  try {
    const planPath = join(out, 'plan.json');
    const notice = [{ kind: 'visible', subject: { testId: 'notice' } }];
    await writeFile(planPath, JSON.stringify({
      schemaVersion: 1,
      kind: 'devup-runtime-audit-plan',
      planId: 'steps',
      plan: {
        baseUrl,
        allowedOrigins: [baseUrl],
        viewports: [{ width: 360, height: 800, deviceScaleFactor: 1 }],
        screens: [
          { id: 'search', routePath: '/steps.html', checks: [{ kind: 'visible', subject: { role: 'textbox', name: 'Search' } }] },
          {
            id: 'result',
            steps: [
              { action: 'fill', target: { role: 'textbox', name: 'Search' }, value: 'school' },
              { action: 'click', target: { role: 'link', name: 'Open' } },
            ],
            expect: { urlPath: '/after.html' },
            checks: notice,
          },
          { id: 'missing', steps: [{ action: 'click', target: { role: 'link', name: 'Nowhere' }, timeoutMs: 500 }], checks: notice },
          { id: 'beyond', steps: [{ action: 'click', target: { role: 'link', name: 'Open' } }], checks: notice },
        ],
      },
    }));
    const report = await runPlan({ plan: planPath, out, maxMinutes: 2 });
    const screens = Object.fromEntries(report.runs[0].screens.map((screen) => [screen.id, screen]));
    assert.deepEqual(report.runs[0].environmentErrors, [], JSON.stringify(report.runs[0], null, 2));
    assert.equal(screens.search.checks[0].status, 'pass');
    assert.equal(screens.result.status, 'completed', JSON.stringify(screens.result, null, 2));
    assert.match(screens.result.url, /\/after\.html$/);
    assert.equal(screens.result.checks[0].status, 'pass');
    assert.equal(screens.missing.status, 'failed');
    assert.match(screens.missing.reason, /^step-timeout/);
    assert.equal(screens.missing.checks[0].status, 'inconclusive');
    assert.equal(screens.beyond.status, 'skipped');
    assert.equal(screens.beyond.checks[0].status, 'skipped');
    assert.equal(report.status, 'partial');
  } finally {
    server.close();
    await rm(out, { recursive: true, force: true });
  }
});

test('a request outside allowedOrigins is blocked and recorded as an environment error', async () => {
  const server = await serve();
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const out = await mkdtemp(join(tmpdir(), 'devup-runtime-audit-'));
  try {
    const planPath = join(out, 'plan.json');
    await writeFile(planPath, JSON.stringify({
      schemaVersion: 1,
      kind: 'devup-runtime-audit-plan',
      planId: 'blocked',
      plan: {
        baseUrl,
        allowedOrigins: ['http://127.0.0.1:1'],
        viewports: [{ width: 360, height: 800, deviceScaleFactor: 1 }],
        screens: [{ id: 'before', routePath: '/before.html', checks: [{ kind: 'no-horizontal-overflow' }] }],
      },
    }));
    const report = await runPlan({ plan: planPath, out, maxMinutes: 2 });
    const errors = report.runs[0].environmentErrors.map((error) => error.kind);
    assert.ok(errors.includes('request-blocked'), JSON.stringify(report.runs[0], null, 2));
    assert.notEqual(report.runs[0].screens[0].status, 'completed');
  } finally {
    server.close();
    await rm(out, { recursive: true, force: true });
  }
});

test('dimensions across 1440/1100/858/390 retain CSS pixels at DPR 2 and filter tolerances', async () => {
  const server = await serve();
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const out = await mkdtemp(join(tmpdir(), 'devup-runtime-dimensions-'));
  const widths = [1440, 1100, 858, 390];
  try {
    const mappings = widths.map((viewportWidth) => ({ viewportWidth, artifactId: 'synthetic-acquisition', nodeId: `target-${viewportWidth}` }));
    const checks = [
      { kind: 'dimensions', label: 'within tolerance', subject: { testId: 'target' }, tolerancePx: 0.5, figmaDimensions: mappings },
      { kind: 'dimensions', label: 'exceeds tolerance', subject: { testId: 'target' }, tolerancePx: 0.49, figmaDimensions: mappings },
      { kind: 'dimensions', label: 'missing element', subject: { testId: 'absent' }, tolerancePx: 1, figmaDimensions: mappings },
      { kind: 'dimensions', label: 'no intermediate reference', subject: { testId: 'target' }, tolerancePx: 1, figmaDimensions: [mappings[0]] },
    ];
    const planPath = join(out, 'plan.json');
    await writeFile(planPath, JSON.stringify({
      schemaVersion: 1, kind: 'devup-runtime-audit-plan', planId: 'synthetic-dimensions',
      plan: { baseUrl, allowedOrigins: [baseUrl], viewports: widths.map((width) => ({ width, height: 240, deviceScaleFactor: 2 })),
        screens: [{ id: 'dimensions', routePath: '/dimensions.html', checks }] },
      // Synthetic acquired snapshots for executor testing; ingest independently rechecks these.
      dimensionReferences: checks.flatMap((check, index) => check.figmaDimensions.map((r) => ({
        ...r, screen: 'dimensions', check: index, status: 'acquired', width: r.viewportWidth / 2 - 20.5, height: 80,
      }))),
    }));
    const report = await runPlan({ plan: planPath, out, maxMinutes: 2 });
    assert.equal(report.status, 'completed');
    const table = [];
    for (const run of report.runs) {
      assert.deepEqual(run.environmentErrors, []);
      const screen = run.screens[0];
      assert.deepEqual(screen.checks.map((c) => c.status), ['pass', 'fail', 'inconclusive', run.viewport.width === 1440 ? 'pass' : 'inconclusive']);
      assert.equal(screen.checks[0].measured.subject.width, run.viewport.width / 2 - 20);
      assert.equal(screen.checks[0].measured.units, 'css-px');
      const png = await readFile(join(out, screen.screenshot.path));
      assert.equal(png.readUInt32BE(16), run.viewport.width * 2, 'DPR changes screenshot pixels only');
      table.push({ width: run.viewport.width, figmaWidth: run.viewport.width / 2 - 20.5,
        domWidth: screen.checks[0].measured.subject.width, delta: screen.checks[0].deltaPx.width,
        exceedances: screen.checks.filter((c) => c.status === 'fail').length });
    }
    assert.deepEqual(table.map((r) => r.exceedances), [1, 1, 1, 1]);
    assert.deepEqual(report.summary, { pass: 5, fail: 4, inconclusive: 7, skipped: 0 });
    console.log(JSON.stringify({ dimensionTable: table }));
  } finally {
    server.close();
    await rm(out, { recursive: true, force: true });
  }
});

test('a view-transition check sees same-document and cross-document transitions, and their absence', async () => {
  const server = await serve();
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const out = await mkdtemp(join(tmpdir(), 'devup-runtime-audit-vt-'));
  try {
    const planPath = join(out, 'plan.json');
    const check = { kind: 'view-transition', label: 'next', subject: { testId: 'go' } };
    await writeFile(planPath, JSON.stringify({
      schemaVersion: 1,
      kind: 'devup-runtime-audit-plan',
      planId: 'browser-vt',
      plan: {
        baseUrl,
        allowedOrigins: [baseUrl],
        viewports: [{ width: 1200, height: 800, deviceScaleFactor: 1 }],
        screens: [
          { id: 'same', routePath: '/transition.html', checks: [{ kind: 'no-horizontal-overflow', label: 'page' }, check] },
          { id: 'cross', routePath: '/crossdoc.html', checks: [check] },
          { id: 'plain', routePath: '/plain.html', checks: [check] },
        ],
        locale: null,
        timezone: null,
        readiness: null,
        stepTimeoutMs: null,
      },
    }));
    const report = await runPlan({ plan: planPath, out, maxMinutes: 5 });
    const [same, cross, plain] = report.runs[0].screens;
    assert.deepEqual(same.checks.map((item) => [item.index, item.status]), [[0, 'pass'], [1, 'pass']], JSON.stringify(same, null, 2));
    assert.equal(cross.checks[0].status, 'pass', JSON.stringify(cross, null, 2));
    assert.equal(plain.checks[0].status, 'fail', JSON.stringify(plain, null, 2));
  } finally {
    server.close();
    await rm(out, { recursive: true, force: true });
  }
});

test('page health, bounds, clipped text, interaction states and captures in a real browser', async () => {
  const server = await serve();
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const out = await mkdtemp(join(tmpdir(), 'devup-runtime-audit-kinds-'));
  try {
    const planPath = join(out, 'plan.json');
    const node = (id) => ({ css: `[data-figma-node-id="${id}"]` });
    await writeFile(planPath, JSON.stringify({
      schemaVersion: 1,
      kind: 'devup-runtime-audit-plan',
      planId: 'browser-kinds',
      plan: {
        baseUrl,
        allowedOrigins: [baseUrl],
        viewports: [{ width: 1200, height: 800, deviceScaleFactor: 1 }, { width: 800, height: 800, deviceScaleFactor: 1 }],
        screens: [
          { id: 'layout', routePath: '/layout.html', checks: [
            { kind: 'page-health', label: 'health' },
            { kind: 'bounds', label: 'card 1', subject: node('1:3'), container: node('1:2'), expected: { x: 20, y: 20, width: 160, height: 100 } },
            { kind: 'bounds', label: 'card 2', subject: node('1:4'), container: node('1:2'), expected: { x: 200, y: 20, width: 160, height: 100 } },
            { kind: 'no-clipped-text', label: 'text', subject: node('1:6') },
            { kind: 'interaction', label: 'cta hover', state: 'hover', subject: { role: 'button', name: 'Send' } },
            { kind: 'interaction', label: 'cta focus', state: 'focus', subject: { role: 'button', name: 'Send' } },
            { kind: 'interaction', label: 'flat hover', state: 'hover', subject: { role: 'link', name: 'Plain link' } },
            { kind: 'capture', label: 'icon', subject: node('1:5'), widths: [1200] },
            { kind: 'interaction', label: 'sweep', state: 'hover' },
          ] },
          { id: 'broken', routePath: '/broken.html', checks: [{ kind: 'page-health', label: 'health' }] },
        ],
        locale: null,
        timezone: null,
        readiness: null,
        stepTimeoutMs: null,
      },
    }));
    const report = await runPlan({ plan: planPath, out, maxMinutes: 5 });
    const [wide, narrow] = report.runs;
    const statuses = (screen) => Object.fromEntries(screen.checks.map((item) => [item.label, item.status]));
    assert.deepEqual(statuses(wide.screens[0]), {
      health: 'pass', 'card 1': 'pass', 'card 2': 'fail', text: 'fail',
      'cta hover': 'pass', 'cta focus': 'pass', 'flat hover': 'fail', icon: 'pass', sweep: 'fail',
    }, JSON.stringify(wide.screens[0].checks, null, 2));
    const card = wide.screens[0].checks.find((item) => item.label === 'card 2');
    assert.match(card.reason, /^position: /);
    const sweep = wide.screens[0].checks.find((item) => item.label === 'sweep');
    assert.match(sweep.reason, /nothing on 1 of 2: <a> "Plain link"/);
    const icon = wide.screens[0].checks.find((item) => item.label === 'icon');
    assert.ok(icon.screenshot.path.endsWith('-c7.png'));
    assert.ok(!narrow.screens[0].checks.some((item) => item.label === 'icon'), 'a capture runs only at its widths');
    assert.equal(statuses(wide.screens[1]).health, 'fail');
    assert.match(wide.screens[1].checks[0].reason, /couldn't load/);
  } finally {
    server.close();
    await rm(out, { recursive: true, force: true });
  }
});

test('a page merged from two width frames is found by its per-width node ids', async () => {
  const server = await serve();
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const out = await mkdtemp(join(tmpdir(), 'devup-runtime-audit-merged-'));
  try {
    const planPath = join(out, 'plan.json');
    const at = (width, id) => ({ css: `[data-figma-node-id="${id}"], [data-figma-node-ids~="${width}:${id}"]` });
    await writeFile(planPath, JSON.stringify({
      schemaVersion: 1,
      kind: 'devup-runtime-audit-plan',
      planId: 'browser-merged',
      plan: {
        baseUrl,
        allowedOrigins: [baseUrl],
        viewports: [{ width: 1440, height: 800, deviceScaleFactor: 1 }, { width: 390, height: 800, deviceScaleFactor: 1 }],
        screens: [{ id: 'merged', routePath: '/merged.html', checks: [
          { kind: 'bounds', label: 'hero 1440', subject: at(1440, 'd:2'), container: at(1440, 'd:1'), expected: { x: 0, y: 0, width: 1440, height: 400 }, widths: [1440] },
          { kind: 'bounds', label: 'hero 390', subject: at(390, 'm:2'), container: at(390, 'm:1'), expected: { x: 0, y: 0, width: 390, height: 300 }, widths: [390] },
          { kind: 'visible', label: 'promo 1440', subject: at(1440, 'd:3'), widths: [1440] },
          { kind: 'hidden', label: 'promo 390', subject: { css: '[data-figma-node-id="d:3"]' }, widths: [390] },
        ] }],
        locale: null,
        timezone: null,
        readiness: null,
        stepTimeoutMs: null,
      },
    }));
    const report = await runPlan({ plan: planPath, out, maxMinutes: 5 });
    const results = report.runs.flatMap((run) => run.screens[0].checks.map((item) => [item.label, item.status, item.reason]));
    assert.deepEqual(results.map(([label, status]) => [label, status]), [
      ['hero 1440', 'pass'], ['promo 1440', 'pass'], ['hero 390', 'pass'], ['promo 390', 'pass'],
    ], JSON.stringify(results));
    assert.ok(!results.some(([, , reason]) => /matched no element/.test(reason)), 'no missing nodes');
    assert.ok(report.runs[0].screens[0].links.includes('/plain.html'), JSON.stringify(report.runs[0].screens[0].links));
  } finally {
    server.close();
    await rm(out, { recursive: true, force: true });
  }
});


test('computed button backgrounds and every image resource are observed in Chromium', async () => {
  const { chromium } = await import('playwright');
  const { evaluateNoLowContrastText, evaluateImageLoaded, evaluateRenderedCompleteness } = await import('../checks.mjs');
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    await page.route('**/missing.svg', (route) => route.fulfill({ status: 404, body: '' }));
    const good = 'data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>';
    await page.setContent(`<button style="color:white">Submit</button>
      <div style="height:100px;background-image:url(https://example.test/missing.svg)"></div>
      <img src='${good}'><img src="https://example.test/missing.svg" loading="lazy">
      <style>body::before { content: ''; background-image: url(https://example.test/missing.svg); }</style>`);
    const observed = await observeCompleteness(page, {}, 800);
    assert.equal(evaluateNoLowContrastText({}, observed).status, 'fail');
    const buttonBackground = await page.locator('button').evaluate((element) => getComputedStyle(element).backgroundColor);
    assert.equal(observed.texts[0].backgrounds[0], buttonBackground);
    assert.notEqual(buttonBackground, 'rgba(0, 0, 0, 0)');
    assert.equal(observed.resources.length, 4);
    assert.equal(observed.resources.filter((item) => !item.loaded).length, 3);
    assert.equal(evaluateImageLoaded(observed).status, 'fail');
    assert.equal(evaluateRenderedCompleteness({ frameHeight: 100 }, observed).status, 'fail');
    await page.setContent(`<button style="color:white;background:black">Submit</button><img src='${good}'>`);
    const fixed = await observeCompleteness(page, {}, 800);
    assert.equal(evaluateNoLowContrastText({}, fixed).status, 'pass');
    assert.equal(evaluateImageLoaded(fixed).status, 'pass');
  } finally { await browser.close(); }
});

test('white outline no-fill button overrides the reset and retains contrast over a dark hero', async () => {
  const { chromium } = await import('playwright');
  const { evaluateNoLowContrastText } = await import('../checks.mjs');
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    await page.setContent(`<style>
      button { -webkit-appearance: button; }
      .hero { background: #101820; padding: 40px; }
      .outline { color: white; border: 1px solid white; background: transparent;
        appearance: none; font-size: 16px; padding: 12px 24px; }
    </style><div class="hero"><button class="outline">Get started</button></div>`);
    const style = await page.locator('button').evaluate((element) => {
      const css = getComputedStyle(element);
      return { appearance: css.appearance, webkitAppearance: css.webkitAppearance,
        background: css.backgroundColor, border: css.borderTopWidth, color: css.color };
    });
    assert.deepEqual(style, { appearance: 'none', webkitAppearance: 'none',
      background: 'rgba(0, 0, 0, 0)', border: '1px', color: 'rgb(255, 255, 255)' });
    // Computed transparent alone did not establish what the native face painted.
    // Sample the rendered padding, away from the white border and glyphs.
    const pixels = await page.locator('button').screenshot();
    const paintedBackground = await page.evaluate(async (base64) => {
      const image = new Image();
      image.src = `data:image/png;base64,${base64}`;
      await image.decode();
      const canvas = document.createElement('canvas');
      canvas.width = image.width;
      canvas.height = image.height;
      const context = canvas.getContext('2d');
      context.drawImage(image, 0, 0);
      return [...context.getImageData(4, 4, 1, 1).data];
    }, pixels.toString('base64'));
    assert.deepEqual(paintedBackground, [16, 24, 32, 255]);
    assert.equal(evaluateNoLowContrastText({}, await observeCompleteness(page, {}, 800)).status, 'pass');
    await page.locator('.hero').evaluate((element) => { element.style.background = 'white'; });
    assert.equal(evaluateNoLowContrastText({}, await observeCompleteness(page, {}, 800)).status, 'fail');
  } finally { await browser.close(); }
});
