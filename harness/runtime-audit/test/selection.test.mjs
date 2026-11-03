import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { chromium } from 'playwright';
import { assertSelection } from '../selection-checks.mjs';
import { collectMarkup, evaluateMarkup } from '../markup-checks.mjs';
import { runPlan } from '../run.mjs';

const locate = (page, target) => target.role ? page.getByRole(target.role, { name: target.name, exact: true }) : page.locator(target.css);
const selected = name => ({ group: { role: 'navigation', name: 'View' }, target: { role: 'link', name },
  attribute: 'aria-current', value: 'true', styles: { 'background-color': 'rgb(0, 87, 187)' } });

test('selected ARIA without matching paint fails; hidden duplicates do not satisfy or break the assertion', async () => {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.setContent('<nav aria-label="View"><a href="/" aria-current="true">List</a><a hidden aria-current="true">Hidden</a></nav>');
    await assert.rejects(assertSelection(page, selected('List'), locate, 80), /paint are inconsistent/);
    await page.locator('a').first().evaluate(el => { el.style.backgroundColor = '#0057bb'; });
    const actual = await assertSelection(page, selected('List'), locate, 100);
    assert.equal(actual.selectedCount, 1);
    await page.locator('nav').evaluate(el => { const duplicate = document.createElement('a'); duplicate.setAttribute('aria-current', 'true'); duplicate.textContent = 'Other'; el.append(duplicate); });
    await assert.rejects(assertSelection(page, selected('List'), locate, 80), /paint are inconsistent/);
  } finally { await browser.close(); }
});

test('runtime plans verify selection through keyboard, back, forward and reload', async () => {
  const server = createServer((request, response) => {
    const list = new URL(request.url, 'http://local').searchParams.get('view') === 'list';
    response.writeHead(200, { 'content-type': 'text/html' }).end(`<html><body><nav aria-label="View"><a href="/?view=thumbnail" ${!list ? 'aria-current="true" style="background:#0057bb"' : ''}>Thumbnail</a><a href="/?view=list" ${list ? 'aria-current="true" style="background:#0057bb"' : ''}>List</a></nav><h1>${list ? 'List' : 'Thumbnail'}</h1></body></html>`);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const out = await mkdtemp(join(tmpdir(), 'devup-selected-'));
  try {
    const plan = join(out, 'plan.json');
    const screen = (id, steps, name) => ({ id, steps, checks: [{ kind: 'page-health' }], expect: { heading: name, selected: selected(name) } });
    await writeFile(plan, JSON.stringify({ schemaVersion: 1, kind: 'devup-runtime-audit-plan', planId: 'selection', plan: {
      baseUrl, allowedOrigins: [baseUrl], viewports: [{ width: 360, height: 800 }], screens: [
        { ...screen('thumbnail', [], 'Thumbnail'), routePath: '/?view=thumbnail' },
        screen('list', [{ action: 'click', target: { role: 'link', name: 'List' } }], 'List'),
        screen('back', [{ action: 'back' }], 'Thumbnail'),
        screen('forward', [{ action: 'forward' }], 'List'),
        screen('reload', [{ action: 'reload' }], 'List'),
        screen('keyboard', [{ action: 'press', target: { role: 'link', name: 'Thumbnail' }, value: 'Enter' }], 'Thumbnail'),
      ],
    } }));
    const report = await runPlan({ plan, out, maxMinutes: 1 });
    for (const row of report.runs[0].screens) {
      assert.equal(row.status, 'completed', JSON.stringify(row));
      assert.equal(row.expectationEvidence.selected.selectedCount, 1);
    }
  } finally { server.close(); await rm(out, { recursive: true, force: true }); }
});

test('markup detects floating short-page footer and accepts a growing server shell', async () => {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 800, height: 900 } });
    await page.setContent('<html lang="en"><body><header>Header</header><nav>Navigation</nav><main><h1>Title</h1></main><footer>Footer</footer></body></html>');
    assert.match(evaluateMarkup(await page.evaluate(collectMarkup)).reason, /short-page footer/);
    await page.addStyleTag({ content: 'body{margin:0;min-height:100vh;display:flex;flex-direction:column}main{flex:1}' });
    assert.equal(evaluateMarkup(await page.evaluate(collectMarkup)).status, 'pass');
  } finally { await browser.close(); }
});
