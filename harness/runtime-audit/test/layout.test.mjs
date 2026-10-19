import assert from 'node:assert/strict';
import test from 'node:test';
import { evaluatePageHealth, evaluateTextAlignment, evaluateSequenceOrder, evaluateSectionLayout } from '../checks.mjs';
import { observeSequence, observeSectionLayout, observeClippedContent, runPlan } from '../run.mjs';

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

test('runner executes the new checks at every required width', async () => {
  const { createServer } = await import('node:http');
  const { mkdtemp, writeFile, rm } = await import('node:fs/promises');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const server = createServer((request, response) => {
    response.setHeader('content-type', 'text/html; charset=utf-8');
    response.end(`<html style="width:300px"><body><main><section style="width:100px"><div>01</div><div>03</div><div>02</div><p>This page couldn't load</p><p data-testid="title" style="text-align:left">Title</p><div id="tech" style="overflow:hidden;width:80px"><div style="width:200px">Devup UI / Vespera</div></div></section><button style="color:white">Submit</button><div style="height:100px;background-image:url(/hero.png)"></div></main></body></html>`);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const dir = await mkdtemp(join(tmpdir(), 'devup-layout-'));
  try {
    const baseUrl = `http://127.0.0.1:${server.address().port}`;
    const path = join(dir, 'plan.json');
    const checks = ['page-health','sequence-order','section-layout','no-clipped-content','no-low-contrast-text','image-loaded'].map((kind) => ({ kind, label: kind }));
    checks.push({ kind: 'text-align', label: 'title alignment', subject: { testId: 'title' }, edge: 'center' });
    await writeFile(path, JSON.stringify({ schemaVersion: 1, kind: 'devup-runtime-audit-plan', planId: 'layout', plan: { baseUrl, allowedOrigins: [baseUrl], viewports: [390,858,1100,1440].map((width) => ({ width, height: 900, deviceScaleFactor: 1 })), screens: [{ id: 'home', routePath: '/', checks }] } }));
    const report = await runPlan({ plan: path, out: join(dir, 'out'), maxMinutes: 1 });
    for (const run of report.runs) {
      for (const check of run.screens[0].checks) assert.equal(check.status, 'fail', `${run.viewport.width}: ${JSON.stringify(check)}`);
      const health = run.screens[0].checks.find((check) => check.kind === 'page-health');
      assert.match(health.reason, /couldn't load/);
      assert.match(health.reason, /300px wide/);
    }
  } finally {
    await new Promise((resolve) => server.close(resolve));
    await rm(dir, { recursive: true, force: true });
  }
});


test('390px header navigation clipping fails until the narrow menu replaces it',async()=> {
  const {chromium}=await import('playwright');
  const browser=await chromium.launch();
  try {
    const page=await browser.newPage({viewport:{width:390,height:900}});
    await page.setContent('<body style="margin:0"><header style="width:390px;overflow:hidden"><nav id="header-nav" style="display:flex;width:600px"><a href="/">Home</a><a href="/company">Company</a><a href="/services">Services</a></nav></header></body>');
    assert.ok((await observeClippedContent(page)).clipped.length>0);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth),390);
    await page.setContent('<header><button aria-expanded="false" aria-controls="drawer">Menu</button><nav id="drawer" hidden><a href="/">Home</a><a href="/company">Company</a></nav></header>');
    assert.equal((await observeClippedContent(page)).clipped.length,0);
  } finally {await browser.close();}
});
