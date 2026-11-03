import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import { randomUUID } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { aggregateQuery, evaluateDataReceipt, validateDataConfig, runDataBindingCheck, DATA_PHASES } from '../data-checks.mjs';

test('typed aggregates parameterize values and refuse injectable table names', () => {
  assert.deepEqual(aggregateQuery({ aggregate: 'count', table: 'patent', field: 'id', where: { kind: 'registered' } }), { query: 'SELECT COUNT("id") AS value FROM "patent" WHERE "kind" = ?', params: ['registered'] });
  assert.throws(() => aggregateQuery({ aggregate: 'count', table: 'patent;DELETE', field: 'id' }), /identifier/);
});
test('API-only receipts cannot claim a DB observation', () => {
  const result = evaluateDataReceipt({ bindings: [{ id: 'total', sourceAnchor: { openapiSha256: 'yes', dbSchemaSha256: 'yes' }, phases: DATA_PHASES.map((phase) => ({ phase, api: { status: 200, body: { total: 3 }, pointer: '/total' }, views: [] })) }] });
  assert.equal(result.verified, false); assert.equal(result.dbVerified, false);
});
test('real disposable SQLite API scenarios update landing/list/detail including empty/error', { timeout: 45000 }, async () => {
  const root = await mkdtemp(join(tmpdir(), 'devup-isolated-data-'));
  const project = join(root, 'project'); await mkdir(join(project, '.devup'), { recursive: true });
  const databasePath = join(root, 'db.sqlite'); const db = new DatabaseSync(databasePath);
  db.exec('CREATE TABLE patent (id INTEGER PRIMARY KEY)');
  const nonce = randomUUID(); let phase = 'baseline'; let mutationCount = 0;
  const count = () => db.prepare('SELECT COUNT(id) AS value FROM patent').get().value;
  const server = createServer(async (request, response) => {
    response.setHeader('content-type', 'application/json');
    if (request.url === '/isolation') { response.end(JSON.stringify({ nonce, databasePath })); return; }
    if (request.url === '/scenario' && request.method === 'POST') {
      if (request.headers['x-devup-isolation'] !== nonce) { response.writeHead(403); response.end('{}'); return; }
      let body = ''; for await (const chunk of request) body += chunk; phase = JSON.parse(body).phase; mutationCount += 1;
      db.exec('DELETE FROM patent'); for (let id = 0; id < ({ baseline: 2, mutated: 5, empty: 0, error: 0 }[phase]); id += 1) db.prepare('INSERT INTO patent VALUES (?)').run(id);
      response.end('{}'); return;
    }
    if (request.url === '/patents') { response.writeHead(phase === 'error' ? 503 : 200); response.end(JSON.stringify({ total: count() })); return; }
    response.setHeader('content-type', 'text/html'); response.end(`<html lang="en"><main><output>${phase === 'error' ? 'Unavailable' : phase === 'empty' ? 'No patents' : `${count()} patents`}</output></main></html>`);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const openapi = { paths: { '/patents': { get: { operationId: 'listPatents', responses: { 200: { content: { 'application/json': { schema: { type: 'object', properties: { total: { type: 'integer' } } } } } } } } }, '/scenario': { post: { operationId: 'setScenario' } } } };
  await writeFile(join(project, 'openapi.json'), JSON.stringify(openapi));
  await writeFile(join(project, 'patent.json'), JSON.stringify({ name: 'patent', columns: [{ name: 'id' }] }));
  await writeFile(join(project, 'Screen.tsx'), 'export function Screen(){return <Text>{data.total}</Text>}');
  await writeFile(join(root, '.devup-isolated.json'), JSON.stringify({ kind: 'devup-isolated-data-test', nonce, databasePath, apiOrigin: origin, appOrigin: origin }));
  const config = { projectRoot: project, openapiPath: 'openapi.json', appBaseUrl: origin, isolation: { disposableRoot: root, databasePath, apiBaseUrl: origin, handshakePath: '/isolation' }, bindings: [{ id: 'patents.total', operationId: 'listPatents', api: { path: '/patents', pointer: '/total' }, db: { modelPath: 'patent.json', table: 'patent', field: 'id', aggregate: 'count' }, phases: DATA_PHASES.map((phase) => ({ phase, mutation: { operationId: 'setScenario', method: 'POST', path: '/scenario', body: { phase } } })), views: ['landing', 'list', 'detail'].map((surface) => ({ surface, route: `/${surface}`, selector: 'output', format: '{value} patents', emptyText: 'No patents', errorText: 'Unavailable' })) }] };
  let browser;
  try {
    const { chromium } = await import('playwright'); browser = await chromium.launch({ headless: true });
    const context = await browser.newContext(); const page = await context.newPage();
    const result = await runDataBindingCheck({ dataBinding: config }, page, { out: join(project, '.devup'), viewport: { width: 390 }, planId: 'test-plan', planSha256: 'test-plan-hash' });
    assert.equal(result.status, 'pass', JSON.stringify(result)); assert.equal(result.measured.dbVerified, true); assert.equal(mutationCount, 5);
    const receipt = JSON.parse(await readFile(join(project, '.devup', result.measured.receipt), 'utf8'));
    assert.deepEqual(receipt.bindings[0].phases.map((phase) => phase.db.rows[0].value), [2, 5, 0, 0]);
    assert.ok(receipt.bindings[0].phases.every((phase) => phase.views.length === 3 && phase.db.snapshot.sha256));
    await assert.rejects(validateDataConfig({ ...config, isolation: { ...config.isolation, disposableRoot: project } }), /inside disposableRoot|marker|ENOENT/);
  } finally { await browser?.close(); await new Promise((resolve) => server.close(resolve)); db.close(); await rm(root, { recursive: true, force: true }); }
});
