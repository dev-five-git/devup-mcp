// input-focus: a field's focus must mark the box the user sees, by click and
// by Tab, with no inner default outline, no movement and focus left on the
// field. Unit cases pin the judgement; the browser case runs fixtures/fields.html.

import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { evaluateInputFocus, judgeFocus, observeInputFocus, snappedProperties } from '../input-focus-checks.mjs';
import { runPlan } from '../run.mjs';

const FIXTURES = new URL('../fixtures/', import.meta.url);

const paint = (overrides = {}) => ({
  borderTopColor: 'rgb(196, 196, 196)', borderRightColor: 'rgb(196, 196, 196)', borderBottomColor: 'rgb(196, 196, 196)', borderLeftColor: 'rgb(196, 196, 196)',
  borderTopWidth: '1px', borderRightWidth: '1px', borderBottomWidth: '1px', borderLeftWidth: '1px',
  boxShadow: 'none', outlineStyle: 'none', outlineWidth: '0px', outlineColor: 'rgb(0, 0, 0)', backgroundColor: 'rgb(255, 255, 255)',
  ...overrides,
});
const rect = (x = 0, y = 0, width = 320, height = 40) => ({ x, y, width, height });
const state = ({ field = {}, container = {}, focus = 'field', fieldRect = rect(28, 0, 280, 40), boxRect = rect(), neighbour = rect(0, 64) } = {}) => ({
  field: { paint: paint({ borderTopWidth: '0px', ...field }), rect: fieldRect },
  container: { paint: paint(container), rect: boxRect },
  neighbour,
  focus,
});
const ring = { borderTopColor: 'rgb(0, 87, 187)', boxShadow: 'rgb(0, 87, 187) 0px 0px 0px 2px' };
const outline = { outlineStyle: 'auto', outlineWidth: '1px', outlineColor: 'rgb(16, 16, 16)' };
const wrapped = { tag: 'input', separateContainer: true, containerTag: 'div' };
const own = { tag: 'input', separateContainer: false, containerTag: 'input' };

test('a field in a box passes only when the box shows focus and the field adds no outline', () => {
  const before = state();
  assert.deepEqual(judgeFocus(wrapped, { before, after: state({ container: ring }) }), []);
  assert.match(judgeFocus(wrapped, { before, after: state({ field: outline }) })[0], /marks only the inner input, not the <div> box/);
  assert.match(judgeFocus(wrapped, { before, after: state({ container: ring, field: outline }) })[0], /second outline shows on the inner input/);
  assert.match(judgeFocus(wrapped, { before, after: state() })[0], /no focus indicator: neither the input nor its <div> box changes/);
});

test('a field that is its own box passes on its own ring; focus elsewhere or movement fails', () => {
  const before = state({ container: {} });
  assert.deepEqual(judgeFocus(own, { before, after: state({ container: ring }) }), []);
  assert.match(judgeFocus(own, { before, after: state() })[0], /nothing on the field changes/);
  assert.match(judgeFocus(wrapped, { before, after: state({ container: ring, focus: 'container' }) })[0], /moved to the container/);
  assert.match(judgeFocus(wrapped, { before, after: state({ container: ring, focus: 'button' }) })[0], /did not reach the field/);
  assert.match(judgeFocus(own, { before, after: state({ container: { ...ring, borderTopWidth: '3px' }, boxRect: rect(0, 0, 324, 44) }) })[0], /changes size or position/);
  assert.match(judgeFocus(own, { before, after: state({ container: ring, neighbour: rect(0, 66) }) })[0], /element after the field moves/);
  assert.match(judgeFocus(wrapped, { before, after: state({ container: ring }), containerTabStop: true })[0], /Tab stops on a container/);
});

test('a focus state that changes paint no transition covers snaps', () => {
  const changedProps = ['borderTopColor', 'borderLeftColor', 'boxShadow', 'outlineStyle'];
  assert.deepEqual(snappedProperties(changedProps, { property: 'all', duration: '0s' }), ['borderTopColor', 'borderLeftColor', 'boxShadow']);
  assert.deepEqual(snappedProperties(changedProps, { property: 'all', duration: '0.16s' }), []);
  assert.deepEqual(snappedProperties(changedProps, { property: 'border-color, box-shadow', duration: '0.16s' }), []);
  assert.deepEqual(snappedProperties(changedProps, { property: 'border', duration: '0.2s' }), ['boxShadow']);
  assert.deepEqual(snappedProperties(changedProps, { property: 'box-shadow, border-color', duration: '0.2s, 0s' }), ['borderTopColor', 'borderLeftColor']);
  assert.deepEqual(snappedProperties(changedProps, undefined), []);
  const snaps = { mode: 'click', before: state(), after: state({ container: ring }) };
  snaps.after.container.motion = { property: 'all', duration: '0s' };
  assert.match(judgeFocus(wrapped, snaps).join(), /snaps with no transition on border-color, box-shadow/);
});

test('the evaluation fails on any field and mode, counts disabled fields, and is inconclusive when nothing could be judged', () => {
  const pass = { mode: 'click', before: state(), after: state({ container: ring }) };
  const fail = { mode: 'tab', before: state(), after: state({ field: outline }) };
  const fields = [
    { ...wrapped, index: 0, name: 'Search', modes: [pass, { ...pass, mode: 'tab' }] },
    { ...own, index: 1, name: 'Locked', disabled: true, modes: [] },
  ];
  const ok = evaluateInputFocus({ fields });
  assert.equal(ok.status, 'pass', ok.reason);
  assert.match(ok.reason, /all 1 field\(s\).*1 disabled field/);
  assert.equal(ok.measured.disabled, 1);
  const bad = evaluateInputFocus({ fields: [{ ...fields[0], modes: [pass, fail] }] });
  assert.equal(bad.status, 'fail');
  assert.match(bad.reason, /<input> "Search" by tab: focus marks only the inner input/);
  assert.equal(evaluateInputFocus({ fields: [{ ...fields[0], modes: [{ mode: 'tab', unreachable: true }] }] }).status, 'inconclusive');
  assert.equal(evaluateInputFocus({ fields: [] }).status, 'pass');
});

function serve() {
  const server = createServer(async (request, response) => {
    const name = new URL(request.url, 'http://local').pathname.slice(1);
    try {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      response.end(await readFile(new URL(name, FIXTURES)));
    } catch {
      response.writeHead(404).end();
    }
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}

test('in a real browser, click and Tab focus are judged per field layout', async () => {
  const server = await serve();
  const { chromium } = await import('playwright');
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 800, height: 900 } });
    await page.goto(`http://127.0.0.1:${server.address().port}/fields.html`);
    const judge = async (id) => evaluateInputFocus(await observeInputFocus(page, page.locator(`#${id}`)));
    const results = {};
    for (const id of ['own', 'wrapped-good', 'wrapped-bad', 'snap', 'wrapped-dup', 'textarea', 'states', 'shift', 'moved', 'select']) {
      results[id] = await judge(id);
    }
    const summary = Object.fromEntries(Object.entries(results).map(([id, result]) => [id, result.status]));
    assert.deepEqual(summary, {
      own: 'pass', 'wrapped-good': 'pass', 'wrapped-bad': 'fail', snap: 'fail', 'wrapped-dup': 'fail', textarea: 'pass',
      states: 'pass', shift: 'fail', moved: 'fail', select: 'pass',
    }, JSON.stringify(results, null, 2));
    // Both modes ran on every enabled field.
    for (const [id, result] of Object.entries(results)) {
      for (const field of result.measured.fields.filter((field) => !field.disabled)) {
        assert.deepEqual(field.modes.map((mode) => mode.mode), ['click', 'tab'], id);
      }
    }
    assert.equal(results['wrapped-good'].measured.fields[0].containerTag, 'div');
    assert.match(results['wrapped-bad'].reason, /by click: focus marks only the inner input, not the <div> box/);
    assert.match(results['wrapped-bad'].reason, /by tab: focus marks only the inner input/);
    assert.match(results['wrapped-dup'].reason, /second outline shows on the inner input/);
    assert.match(results.snap.reason, /by click: the focus state snaps with no transition on border-color, box-shadow/);
    assert.doesNotMatch(results['wrapped-good'].reason, /snaps/);
    assert.match(results.shift.reason, /changes size or position/);
    assert.match(results.moved.reason, /Tab stops on a container/);
    assert.equal(results.states.measured.disabled, 1);
    assert.ok(results.states.measured.fields.some((field) => field.invalid && !field.disabled));
    // The probe leaves no marker behind and the page keeps its values.
    assert.equal(await page.locator('[data-devup-focus-probe]').count(), 0);
    assert.equal(await page.locator('#states input').first().inputValue(), 'not-an-email');
  } finally {
    await browser.close();
    server.close();
  }
});

test('the executor runs an input-focus check from a plan at its widths', async () => {
  const server = await serve();
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const out = await mkdtemp(join(tmpdir(), 'devup-input-focus-'));
  try {
    const plan = join(out, 'plan.json');
    await writeFile(plan, JSON.stringify({ schemaVersion: 1, kind: 'devup-runtime-audit-plan', planId: 'input-focus', plan: {
      baseUrl, allowedOrigins: [baseUrl], viewports: [{ width: 800, height: 900 }, { width: 400, height: 900 }],
      screens: [{ id: 'fields', routePath: '/fields.html', checks: [
        { kind: 'input-focus', label: 'good box', subject: { css: '#wrapped-good' }, widths: [800] },
        { kind: 'input-focus', label: 'bad box', subject: { css: '#wrapped-bad' }, widths: [800] },
        { kind: 'input-focus', label: 'missing', subject: { css: '#nothing' }, widths: [800] },
      ] }],
    } }));
    const report = await runPlan({ plan, out, maxMinutes: 2 });
    const [wide, narrow] = report.runs;
    assert.deepEqual(wide.screens[0].checks.map((check) => [check.label, check.status]), [
      ['good box', 'pass'], ['bad box', 'fail'], ['missing', 'inconclusive'],
    ], JSON.stringify(wide.screens[0].checks, null, 2));
    assert.equal(narrow.screens[0].checks.filter((check) => check.kind === 'input-focus').length, 0);
  } finally {
    server.close();
    await rm(out, { recursive: true, force: true });
  }
});
