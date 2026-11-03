import assert from 'node:assert/strict';
import test from 'node:test';
import { collectMarkup, evaluateMarkup } from '../markup-checks.mjs';
import { observeCompleteness } from '../run.mjs';

test('markup records missing semantics and heading skips without treating target-size exceptions as failures', () => {
  const observed = { bodyPresent: true, lang: 'ko', landmarks: { header: 1, nav: 1, main: 1, footer: 1 },
    headings: [{ level: 1 }, { level: 2 }], missingAlt: [], smallTargets: [{ tag: 'a', inline: true, width: 16, height: 12 }] };
  const passing = evaluateMarkup(observed);
  assert.equal(passing.status, 'pass');
  assert.match(passing.measured.targetSizeReview, /exceptions/);
  const broken = evaluateMarkup({ ...observed, lang: '', landmarks: { ...observed.landmarks, main: 2 },
    headings: [{ level: 2 }, { level: 4 }], missingAlt: [{ tag: 'img' }] });
  assert.equal(broken.status, 'fail');
  assert.match(broken.reason, /language.*one main.*one h1.*heading level skips.*no alt/);
  assert.equal(evaluateMarkup({ bodyPresent: false }).status, 'inconclusive');
});

test('browser probes detect missing alt and use explicit section mappings without Figma attributes', async () => {
  const { chromium } = await import('playwright');
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.setContent('<html lang="ko"><body><header><nav>Menu</nav></header><main id="content"><h1>Title</h1><section id="hero">Hello <span>world</span></section><img width="2" height="2"><img alt="" width="2" height="2"></main><footer>Footer</footer></body></html>');
    const markup = await page.evaluate(collectMarkup);
    assert.equal(markup.missingAlt.length, 1);
    assert.equal(markup.landmarks.main, 1);
    const mapped = await observeCompleteness(page, { sections: ['figma:hero'], nodeSelectors: { 'figma:hero': '#hero' } }, 390);
    assert.equal(mapped.sections[0].count, 1);
    assert.equal(mapped.sections[0].visible, true);
    const absent = await observeCompleteness(page, { sections: ['figma:hero'] }, 390);
    assert.equal(absent.sections[0].count, 0);
    const ambiguous = await observeCompleteness(page, { sections: ['figma:hero'], nodeSelectors: { 'figma:hero': 'header,footer' } }, 390);
    assert.equal(ambiguous.sections[0].count, 2);
  } finally { await browser.close(); }
});

test('aria-hidden landmarks cannot satisfy semantics and presentation on a container does not hide child images', async () => {
  const { chromium } = await import('playwright');
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.setContent(`<html lang="ko"><body>
      <div aria-hidden="true"><header>Header</header><nav>Navigation</nav><main>Hidden main</main><footer>Footer</footer></div>
      <div inert><main>Inert main</main></div>
      <h1>Visible title</h1>
      <div role="none"><img width="20" height="20"></div>
      <div role="presentation"><img width="20" height="20"></div>
      <img role="presentation" width="20" height="20">
      <img aria-hidden="true" width="20" height="20">
    </body></html>`);
    const markup = await page.evaluate(collectMarkup);
    assert.deepEqual(markup.landmarks, { header: 0, nav: 0, main: 0, footer: 0 });
    assert.equal(markup.missingAlt.length, 2);
    const result = evaluateMarkup(markup);
    assert.equal(result.status, 'fail');
    assert.match(result.reason, /main landmark, found 0/);
    assert.match(result.reason, /2 images have no alt/);
  } finally { await browser.close(); }
});
