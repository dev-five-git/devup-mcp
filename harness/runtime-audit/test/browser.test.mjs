// The executor against a real Chromium: the hand-widened screen passes at the
// width it was drawn at and fails each named relation at every other width,
// and the policy-anchored screen keeps every relation at all four widths.

import assert from 'node:assert/strict';
import { readFile, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';
import test from 'node:test';
import { runPlan } from '../run.mjs';

const FIXTURES = new URL('../fixtures/', import.meta.url);
const TYPES = { '.html': 'text/html; charset=utf-8', '.svg': 'image/svg+xml' };

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
