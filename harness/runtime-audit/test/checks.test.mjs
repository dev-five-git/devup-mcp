import assert from 'node:assert/strict';
import test from 'node:test';

test('interaction distinguishes a child-only state change from a dead state', () => {
  const item = { tag: 'a', name: 'Company', before: {}, after: {}, childChanges: ['span'] };
  const result = evaluateInteraction({ state: 'hover' }, { items: [item] });
  assert.equal(result.status, 'fail');
  assert.match(result.reason, /state styles are on a child.*focusable/);
  assert.deepEqual(result.measured.unchanged[0].childChanges, ['span']);
  assert.match(evaluateInteraction({ state: 'focus' }, { subjectCount: 1, ...item }).reason, /state styles are on a child/);
});
import {
  GEOMETRY_ROUNDING_PX, STATE_PROPERTIES, evaluateBounds, evaluateHidden, evaluateCheck, evaluateClippedText, evaluateDimensions, evaluateInteraction,
  evaluatePageHealth, evaluateNoLowContrastText, evaluateImageLoaded, evaluateRenderedCompleteness, evaluateViewTransition, summarize, textContrast,
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
  const check = { kind: 'bounds', expected: { x: 20, y: 20, width: 160, height: 100 } };
  const at = (x, y, width, height) => ({ subjectCount: 1, containerCount: 1, subject: box(100 + x, 50 + y, width, height), container });
  assert.equal(evaluateBounds(check, at(20.4, 19.6, 160.4, 100)).status, 'pass', 'rounding is not a mismatch');
  const onePixel = evaluateBounds(check, at(21, 20, 160, 100));
  assert.equal(onePixel.status, 'fail', 'layout is exact: 1px off at a drawn width fails');
  assert.match(onePixel.reason, /^position: x off by 1px/);
  assert.equal(evaluateBounds({ ...check, tolerancePx: 50 }, at(21, 20, 160, 100)).status, 'fail', 'no tolerance can be configured');
  assert.equal(GEOMETRY_ROUNDING_PX, 0.5);
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

test('an element a width does not draw must be hidden there', () => {
  assert.equal(evaluateHidden({ subjectCount: 0 }).status, 'pass');
  assert.equal(evaluateHidden({ subjectCount: 1, visible: false }).status, 'pass');
  assert.equal(evaluateHidden({ subjectCount: 1, visible: true }).status, 'fail');
});

test('production builds lose the audit attributes; dev and audit builds keep them', async () => {
  const { default: plugin, stripFigmaNodeIds, keepsAttributes } = await import('../strip-figma-node-ids.mjs');
  const source = '<Box data-figma-node-id="1:2" data-figma-node-ids="1440:1:2 390:3:4" p="20px">\n  <Text data-figma-node-id="1:3">Hi</Text>\n</Box>';
  const stripped = stripFigmaNodeIds(source);
  assert.ok(!stripped.includes('data-figma-node-id'), stripped);
  assert.equal(stripped, '<Box p="20px">\n  <Text>Hi</Text>\n</Box>');
  const production = plugin({});
  assert.equal(production.enforce, 'pre');
  assert.equal(production.apply({}, { command: 'build' }), true);
  assert.equal(production.apply({}, { command: 'serve' }), false, 'a dev server keeps them for the audit');
  assert.equal(plugin({ DEVUP_AUDIT: '1' }).apply({}, { command: 'build' }), false, 'DEVUP_AUDIT=1 is an audit build');
  assert.equal(production.transform(source, '/app/src/app/page.tsx').code, stripped);
  assert.equal(production.transform(source, '/app/src/style.css'), null);
  assert.equal(keepsAttributes('build', {}), false);
});

test('a rendered page is complete only when tall enough, with every section, image and visible text', () => {
  const completeness = { frameHeight: 2000, sections: ['1:2', '1:3'], images: ['1:4'] };
  const shown = (id) => ({ id, count: 1, visible: true, width: 100, height: 50, broken: false });
  const dark = { text: 'Hello', tag: 'p', color: 'rgb(17, 17, 17)', backgrounds: ['rgba(0, 0, 0, 0)', 'rgb(255, 255, 255)'], imageBehind: false, opacity: 1 };
  const complete = { pageHeight: 1900, sections: [shown('1:2'), shown('1:3')], images: [shown('1:4')], texts: [dark] };
  assert.equal(evaluateRenderedCompleteness(completeness, complete).status, 'pass');

  const blank = evaluateRenderedCompleteness(completeness, { pageHeight: 900, sections: [{ id: '1:2', count: 0 }, shown('1:3')], images: [{ ...shown('1:4'), broken: true }], texts: [] });
  assert.equal(blank.status, 'fail');
  assert.match(blank.reason, /900px tall, under 90% of the 2000px frame/);
  assert.match(blank.reason, /1 of 2 sections not rendered: 1:2/);
  assert.match(blank.reason, /1 images did not load: 1:4/);

  const whiteOnWhite = { ...dark, text: 'Ghost', color: 'rgb(255, 255, 255)' };
  const ghost = evaluateRenderedCompleteness(completeness, { ...complete, texts: [dark, whiteOnWhite] });
  assert.equal(ghost.status, 'fail');
  assert.match(ghost.reason, /"Ghost" \(1:1\)/);
  assert.equal(textContrast({ ...whiteOnWhite, imageBehind: true }), null, 'text over an image is not judged');
  assert.ok(textContrast({ ...whiteOnWhite, backgrounds: ['rgb(0, 0, 0)'] }) > 20);
  assert.ok(textContrast({ ...dark, opacity: 0 }) < 1.01, 'fully transparent text cannot be seen');
});


test('no-low-contrast-text composites ancestor backgrounds and translucent ink', () => {
  const sample = { text: 'Submit', tag: 'button', color: 'rgb(255, 255, 255)', backgrounds: ['rgb(239, 239, 239)'] };
  const check = (text) => evaluateNoLowContrastText({}, { texts: [text] });
  assert.equal(check(sample).status, 'fail');
  assert.equal(check({ ...sample, backgrounds: ['rgba(0, 0, 0, 0)', 'rgb(0, 0, 0)'] }).status, 'pass');
  assert.equal(check({ ...sample, color: 'rgba(0, 0, 0, 0.01)' }).status, 'fail');
  assert.equal(check({ ...sample, imageBehind: true }).status, 'inconclusive');
  assert.equal(check({ ...sample, color: 'unsupported' }).status, 'inconclusive');
});

test('image-loaded fails for any undecoded img or background resource', () => {
  const resources = [{ source: '/ok.png', kind: 'img', loaded: true }, { source: '/hero.png', kind: 'background', loaded: false }];
  assert.equal(evaluateImageLoaded({ resources: resources.slice(0, 1) }).status, 'pass');
  const broken = evaluateImageLoaded({ resources });
  assert.equal(broken.status, 'fail');
  assert.match(broken.reason, /hero.png/);
  assert.equal(evaluateImageLoaded({ resources: [{ ...resources[0], loaded: false }] }).status, 'fail');
});


test('contrast composites opacity of text and background together through ancestor groups', () => {
  const sample = { color: 'rgb(255, 255, 255)', layers: [{ color: 'rgb(0, 0, 0)', opacity: 0.01 }] };
  assert.ok(textContrast(sample) < 1.5, 'a faded black button on white loses its white text');
  assert.ok(textContrast({ ...sample, layers: [{ color: 'rgb(0, 0, 0)', opacity: 1 }] }) > 20);
  assert.ok(textContrast({ color: 'rgb(0, 0, 0)', layers: [{ color: 'rgba(0, 0, 0, 0)', opacity: 1 }, { color: 'rgb(255, 255, 255)', opacity: 1 }] }) > 20);
});

test('transparent backgrounds use the browser canvas colour', () => {
  assert.ok(textContrast({ color: 'rgb(255, 255, 255)', canvasColor: 'rgb(0, 0, 0)', backgrounds: ['rgba(0, 0, 0, 0)'] }) > 20);
});
