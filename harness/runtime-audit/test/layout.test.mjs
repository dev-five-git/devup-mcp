import assert from 'node:assert/strict';
import test from 'node:test';
import { evaluatePageHealth, evaluateTextAlignment, evaluateSequenceOrder, evaluateSectionLayout } from '../checks.mjs';
import { observeSequence, observeSectionLayout, observeClippedContent } from '../run.mjs';

test('health rejects narrow documents and records vinext error text', () => {
  const healthy = { errorTexts: [], errors: [], viewportWidth: 858, documentWidth: 858 };
  assert.equal(evaluatePageHealth(healthy).status, 'pass');
  assert.match(evaluatePageHealth({ ...healthy, documentWidth: 700 }).reason, /700px wide in a 858px viewport/);
  assert.match(evaluatePageHealth({ ...healthy, errorTexts: ["This page couldn't load"] }).reason, /couldn't load/);
});

test('sequence order follows rows and rejects collapsed column DOM order', () => {
  const group = [1, 3, 2, 4].map((number, index) => ({ number, name: `card ${number}`, x: 0, y: index * 100 }));
  assert.match(evaluateSequenceOrder({ groups: [group] }).reason, /card 3 before card 2/);
  assert.equal(evaluateSequenceOrder({ groups: [group.map((item) => ({ ...item, x: item.number % 2 ? 0 : 200, y: item.number <= 2 ? 0 : 100 }))] }).status, 'pass');
});

test('collapsed sections, hidden sections and squashed text fail', () => {
  const section = { name: 'hero', visible: true, width: 390, height: 300, containerWidth: 390 };
  assert.equal(evaluateSectionLayout({ sections: [section], texts: [] }).status, 'pass');
  assert.match(evaluateSectionLayout({ sections: [{ ...section, width: 150 }], texts: [] }).reason, /hero/);
  assert.equal(evaluateSectionLayout({ sections: [{ ...section, visible: false }], texts: [] }).status, 'fail');
  assert.match(evaluateSectionLayout({ sections: [], texts: [{ name: 'description', width: 30 }] }).reason, /description/);
});

test('browser observes numbered column collapse and squashed sections at each width', async () => {
  const { chromium } = await import('playwright');
  const browser = await chromium.launch({ headless: true });
  try {
    for (const width of [390,858,1100,1440]) {
      const page = await browser.newPage({ viewport: { width, height: 900 } });
      await page.setContent('<main><section style="width:150px"><span>01</span><br><span>03</span><br><span>02</span><br><span>04</span></section></main>');
      assert.equal(evaluateSequenceOrder(await observeSequence(page)).status, 'fail');
      assert.equal(evaluateSectionLayout(await observeSectionLayout(page)).status, 'fail');
      await page.setContent('<main><section><span>STEP 1</span><span>STEP 2</span><span>STEP 3</span></section></main>');
      assert.equal(evaluateSequenceOrder(await observeSequence(page)).status, 'pass');
      await page.close();
    }
  } finally { await browser.close(); }
});

test('internal nav and thumbnail clipping fails even without page overflow', async () => {
  const { chromium } = await import('playwright');
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 858, height: 900 } });
  try {
    await page.setContent('<nav id="nav" style="width:300px;overflow:hidden"><div style="width:700px">Links</div></nav><div id="tech" style="width:200px;overflow:auto;scrollbar-width:none"><div style="width:600px">Devup UI / Vespera</div></div>');
    const observed = await observeClippedContent(page);
    assert.deepEqual(observed.clipped.map((item) => item.name), ['nav','tech']);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth), 858);
    await page.setContent('<div><button aria-label="Next">Next</button><div data-audit-carousel="true" style="width:200px;overflow:auto"><div style="width:600px">Thumbnails</div></div></div>');
    assert.equal((await observeClippedContent(page)).clipped.length, 0);
  } finally { await browser.close(); }
});

test('text alignment follows Figma even when its text box stays full width', () => {
  assert.equal(evaluateTextAlignment({ edge: 'center' }, { count: 1, visible: true, align: 'left', direction: 'ltr' }).status, 'fail');
  assert.equal(evaluateTextAlignment({ edge: 'center' }, { count: 1, visible: true, align: 'center', direction: 'ltr' }).status, 'pass');
});
