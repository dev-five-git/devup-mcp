import assert from 'node:assert/strict';
import test from 'node:test';
import { evaluateCheck, summarize } from '../checks.mjs';

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
