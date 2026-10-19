import assert from 'node:assert/strict';
import test from 'node:test';
import {
  diffLoads, evaluateImagesLoaded, evaluateNativeAppearance, evaluateStaleResources, observeImages, observeNativeAppearance,
} from '../resource-checks.mjs';

const control = (over) => ({ tag: 'button', name: 'Go', appearance: 'none', backgroundColor: 'rgb(0, 0, 255)', authorBackground: true, authorBorder: true, ...over });

test('native appearance fails buttons that keep the browser face', () => {
  assert.equal(evaluateNativeAppearance({ controls: [control()] }).status, 'pass');
  const result = evaluateNativeAppearance({ controls: [control({ appearance: 'button', authorBackground: false })] });
  assert.equal(result.status, 'fail');
  assert.match(result.reason, /<button> "Go"/);
  assert.equal(evaluateNativeAppearance({ controls: [control({ backgroundColor: 'rgb(239, 239, 239)' })] }).status, 'fail');
});

test('images fail when unpainted or their request failed', () => {
  const ok = { kind: 'img', element: 'img', source: 'http://x/a.png', loaded: true, width: 10, height: 10 };
  assert.equal(evaluateImagesLoaded({ images: [ok] }).status, 'pass');
  assert.match(evaluateImagesLoaded({ images: [{ ...ok, width: 0 }] }).reason, /no painted box/);
  const failed = evaluateImagesLoaded({ images: [ok] }, { responses: [{ url: 'http://x/a.png', status: 404 }] });
  assert.equal(failed.status, 'fail');
  assert.match(failed.reason, /404 http:\/\/x\/a\.png/);
  assert.equal(evaluateImagesLoaded({ images: [ok] }, { scoped: true, responses: [{ url: 'http://x/other.png', status: 404 }] }).status, 'pass');
});

test('stale resources compare with a fresh load', () => {
  const load = { stylesheets: [{ href: 'a.css', hash: '1' }], images: ['i.png'], firstSection: { name: 'section', height: 100 } };
  assert.equal(evaluateStaleResources({ responses: [{ url: 'http://x/a.js', status: 200 }], first: load, second: load }).status, 'pass');
  assert.equal(evaluateStaleResources({ responses: [{ url: 'http://x/a.js', status: 404 }], first: load, second: load }).status, 'fail');
  const changed = { ...load, stylesheets: [{ href: 'a.css', hash: '2' }] };
  assert.match(evaluateStaleResources({ responses: [], first: load, second: changed }).reason, /content changed/);
  assert.equal(diffLoads(load, { ...load, firstSection: { name: 'section', height: 300 } }).length, 1);
  assert.equal(evaluateStaleResources({ responses: [], first: load, second: null, reloadError: 'x' }).status, 'inconclusive');
});

test('browser observes button chrome and unpainted images', async () => {
  const { chromium } = await import('playwright');
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    await page.setContent('<button style="appearance:none;background:#00f;border:1px solid #000">A</button><button>B</button>');
    const observed = await observeNativeAppearance(page.locator('body'));
    assert.deepEqual(evaluateNativeAppearance(observed).measured.native.map((item) => item.name), ['B']);
    await page.setContent('<img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" style="width:0"><div style="width:20px;height:20px;background-image:url(data:image/gif;base64,AAAA)"></div>');
    const images = await observeImages(page.locator('body'));
    assert.equal(evaluateImagesLoaded(images).status, 'fail');
  } finally { await browser.close(); }
});
