import assert from 'node:assert/strict';
import test from 'node:test';
import {
  STATE_PROPERTIES, evaluateBounds, evaluateCheck, evaluateClippedText, evaluateDimensions, evaluateInteraction,
  evaluatePageHealth, evaluateViewTransition, summarize,
} from '../checks.mjs';
import { validateDimensionsPlan } from '../run.mjs';

const page = { viewportWidth: 360, scrollWidth: 360, maxRight: 340, overflowHidden: false };
const box = (x, y, width, height) => ({ x, y, width, height });

test('overflow counts content clipped by overflow:hidden, not only scrolling', () => {
  assert.equal(evaluateCheck({ kind: 'no-horizontal-overflow' }, { page }).status, 'pass');
  const clipped = evaluateCheck(
    { kind: 'no-horizontal-overflow' },
    { page: { ...page, maxRight: 400, overflowHidden: true } },
  );
  assert.equal(clipped.status, 'fail');
  assert.match(clipped.reason, /clipped by overflow/);
  const scrolling = evaluateCheck({ kind: 'no-horizontal-overflow' }, { page: { ...page, scrollWidth: 420 } });
  assert.equal(scrolling.status, 'fail');
});

test('containment, alignment and spans measure the named relation only', () => {
  const card = box(20, 193, 1160, 200);
  const one = { subjectCount: 1, containerCount: 1, page };
  assert.equal(evaluateCheck({ kind: 'contained' }, { ...one, subject: box(1100, 210, 64, 22), container: card }).status, 'pass');
  const escaped = evaluateCheck({ kind: 'contained' }, { ...one, subject: box(1150, 210, 64, 22), container: card });
  assert.equal(escaped.status, 'fail');
  assert.match(escaped.reason, /34px on the right/);

  const right = { kind: 'aligned', edge: 'right', offsetPx: 16 };
  assert.equal(evaluateCheck(right, { ...one, subject: box(1116, 210, 48, 48), container: card }).status, 'pass');
  assert.equal(evaluateCheck(right, { ...one, subject: box(276, 210, 48, 48), container: card }).status, 'fail');
  const centre = { kind: 'aligned', edge: 'center' };
  assert.equal(evaluateCheck(centre, { ...one, subject: box(500, 700, 200, 20), container: box(0, 0, 1200, 760) }).status, 'pass');

  const spans = { kind: 'spans', insetPx: 15.5 };
  assert.equal(evaluateCheck(spans, { ...one, subject: box(35.5, 360, 1129, 1), container: card }).status, 'pass');
  assert.equal(evaluateCheck(spans, { ...one, subject: box(455.5, 360, 289, 1), container: card }).status, 'fail');
});

test('a locator that finds nothing, or more than one thing, is inconclusive rather than failed', () => {
  for (const count of [0, 2]) {
    const result = evaluateCheck({ kind: 'visible' }, { subjectCount: count, page });
    assert.equal(result.status, 'inconclusive');
  }
  const noContainer = evaluateCheck(
    { kind: 'contained' },
    { subjectCount: 1, subject: box(0, 0, 1, 1), containerCount: 0, page },
  );
  assert.equal(noContainer.status, 'inconclusive');
});

test('the summary is recomputed from the items', () => {
  const runs = [{ screens: [{ checks: [{ status: 'pass' }, { status: 'fail' }] }, { checks: [{ status: 'skipped' }] }] }];
  assert.deepEqual(summarize(runs), { pass: 1, fail: 1, inconclusive: 0, skipped: 1 });
});

test('dimensions use raw CSS geometry, inclusive per-check tolerance and explicit references', () => {
  const check = { kind: 'dimensions', tolerancePx: 0.25 };
  const measured = { subjectCount: 1, subjectVisible: true, subject: box(0, 0, 100.25, 80) };
  const reference = { status: 'acquired', width: 100, height: 80 };
  assert.equal(evaluateDimensions(check, measured, reference).status, 'pass');
  assert.equal(evaluateDimensions({ ...check, tolerancePx: 0.249 }, measured, reference).status, 'fail');
  assert.equal(evaluateDimensions(check, measured, null).status, 'inconclusive');
  for (const subjectCount of [0, 2]) assert.equal(evaluateDimensions(check, { ...measured, subjectCount }, reference).status, 'inconclusive');
  assert.equal(evaluateDimensions(check, { ...measured, subjectVisible: false }, reference).status, 'inconclusive');
  for (const tolerancePx of [-1, NaN, Infinity, 10001]) assert.throws(() => evaluateDimensions({ ...check, tolerancePx }, measured, reference), /tolerance/);
});

test('dimension runner refuses inferred values, duplicate mappings and invalid tolerances before browser launch', () => {
  const mapping = { viewportWidth: 390, artifactId: 'fixture', nodeId: 'target' };
  const check = { kind: 'dimensions', subject: { testId: 'target' }, tolerancePx: 0, figmaDimensions: [mapping] };
  const plan = (entry) => ({ viewports: [{ width: 390 }], screens: [{ checks: [entry] }] });
  validateDimensionsPlan(plan(check));
  for (const entry of [
    { ...check, tolerancePx: -1 }, { ...check, measure: 'painted-mask' },
    { ...check, figmaDimensions: [mapping, mapping] },
    { ...check, figmaDimensions: [{ ...mapping, width: 100 }] },
    { ...check, figmaDimensions: [{ ...mapping, viewportWidth: 858 }] },
  ]) assert.throws(() => validateDimensionsPlan(plan(entry)));
});

test('a view-transition check passes only when following the link ran one', () => {
  const observed = { subjectCount: 1, supported: true, navigated: true, calls: 1, from: '/a', to: '/b' };
  assert.equal(evaluateViewTransition(observed).status, 'pass');
  assert.equal(evaluateViewTransition({ ...observed, calls: 0 }).status, 'fail');
  assert.match(evaluateViewTransition({ ...observed, calls: 0 }).reason, /ran no view transition/);
  assert.equal(evaluateViewTransition({ ...observed, navigated: false }).status, 'inconclusive');
  assert.equal(evaluateViewTransition({ ...observed, supported: false }).status, 'inconclusive');
  assert.equal(evaluateViewTransition({ subjectCount: 0 }).status, 'inconclusive');
});

test('page health fails on an error page, a page error or a console error', () => {
  assert.equal(evaluatePageHealth({ errorTexts: [], errors: [] }).status, 'pass');
  const broken = evaluatePageHealth({ errorTexts: ["This page couldn't load"], errors: [{ kind: 'page-error', message: 'boom\nstack' }] });
  assert.equal(broken.status, 'fail');
  assert.match(broken.reason, /couldn't load.*page-error: boom$/);
});

test('bounds judge position and size relative to the container', () => {
  const container = box(100, 50, 600, 400);
  const check = { kind: 'bounds', expected: { x: 20, y: 20, width: 160, height: 100 }, tolerancePx: 2 };
  const at = (x, y, width, height) => ({ subjectCount: 1, containerCount: 1, subject: box(100 + x, 50 + y, width, height), container });
  assert.equal(evaluateBounds(check, at(21, 19, 160, 100)).status, 'pass');
  const moved = evaluateBounds(check, at(20, 80, 160, 100));
  assert.equal(moved.status, 'fail');
  assert.match(moved.reason, /^position: y off by 60px/);
  assert.match(evaluateBounds(check, at(20, 20, 300, 100)).reason, /^size: width off by 140px/);
  assert.equal(evaluateBounds(check, { ...at(0, 0, 1, 1), containerCount: 0 }).status, 'inconclusive');
});

test('clipped text fails; nothing clipped passes', () => {
  assert.equal(evaluateClippedText({ clipped: [] }).status, 'pass');
  assert.equal(evaluateClippedText({ clipped: [{ text: 'Long', tag: 'p' }] }).status, 'fail');
});

test('an interaction state passes only when something visible changes', () => {
  const before = Object.fromEntries(STATE_PROPERTIES.map((property) => [property, 'a']));
  assert.equal(evaluateInteraction({ state: 'hover' }, { subjectCount: 1, before, after: { ...before, color: 'b' } }).status, 'pass');
  assert.equal(evaluateInteraction({ state: 'hover' }, { subjectCount: 1, before, after: before }).status, 'fail');
});

test('an interaction sweep fails on the links and buttons that change nothing', () => {
  const before = Object.fromEntries(STATE_PROPERTIES.map((property) => [property, 'a']));
  const items = [
    { tag: 'a', name: 'Home', before, after: { ...before, color: 'b' } },
    { tag: 'button', name: 'Send', before, after: before },
  ];
  const result = evaluateInteraction({ state: 'hover' }, { items });
  assert.equal(result.status, 'fail');
  assert.match(result.reason, /1 of 2: <button> "Send"/);
  assert.equal(evaluateInteraction({ state: 'focus' }, { items: [items[0]] }).status, 'pass');
});
