// Opt-in destructive test scenarios target only a nonce-bound disposable local
// API. SQLite is opened read-only; this module never writes business data to DB.
import { createHash } from 'node:crypto';
import { readFile, realpath, mkdir, writeFile } from 'node:fs/promises';
import { isAbsolute, relative, resolve, join, sep } from 'node:path';
import { tmpdir } from 'node:os';
import { sourceDigest } from './source-digest.mjs';

export const DATA_PHASES = ['baseline', 'mutated', 'empty', 'error'];
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const inside = (root, path) => { const rel = relative(root, path); return rel !== '' && rel !== '..' && !rel.startsWith(`..${sep}`) && !isAbsolute(rel); };
const local = (url) => { const parsed = new URL(url); if (!['http:', 'https:'].includes(parsed.protocol) || !['127.0.0.1', 'localhost', '[::1]'].includes(parsed.hostname) || parsed.username || parsed.password) throw new Error('Data scenarios require explicit loopback origins.'); return parsed; };
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const normalize = (value) => String(value ?? '').replace(/\s+/g, ' ').trim();
const identifier = (name) => { if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) throw new Error('Invalid explicit DB identifier.'); return `"${name}"`; };
export function pointer(body, path) { return path.split('/').slice(1).reduce((value, key) => value?.[key.replaceAll('~1', '/').replaceAll('~0', '~')], body); }
async function sourceFile(root, path) {
  if (!path || isAbsolute(path)) throw new Error('Source anchors must be relative paths.');
  const target = await realpath(resolve(root, path));
  if (!inside(root, target)) throw new Error('Source anchor escapes projectRoot.');
  const bytes = await readFile(target); if (bytes.length > 2 * 1024 * 1024) throw new Error('Source anchor too large.');
  return { path, sha256: hash(bytes), bytes };
}
function operation(spec, operationId, method, actualPath) {
  const matches = [];
  for (const [path, methods] of Object.entries(spec.paths ?? {})) for (const [verb, op] of Object.entries(methods)) {
    const pattern = path.split('/'); const actual = new URL(actualPath, 'http://localhost').pathname.split('/');
    if (op?.operationId === operationId && verb.toUpperCase() === method.toUpperCase() && pattern.length === actual.length && pattern.every((part, i) => /^\{[^}]+\}$/.test(part) || part === actual[i])) matches.push({ path, method: verb, operation: op });
  }
  if (matches.length !== 1) throw new Error(`Existing OpenAPI operation not uniquely matched: ${operationId}`);
  return matches[0];
}
function schemaField(spec, schema, path) {
  const deref = (input) => { let value = input; for (let depth = 0; value?.$ref && depth < 20; depth += 1) value = pointer(spec, value.$ref.slice(1)); return value; };
  let current = deref(schema);
  for (const key of path.split('/').slice(1)) { current = deref(/^\d+$/.test(key) ? current?.items : current?.properties?.[key.replaceAll('~1', '/').replaceAll('~0', '~')]); if (!current) return false; }
  return Boolean(current);
}
export async function validateDataConfig(config) {
  if (!config || !Array.isArray(config.bindings) || !config.bindings.length || config.bindings.length > 20) throw new Error('dataBinding requires 1..20 explicit bindings.');
  const project = await realpath(config.projectRoot); const disposable = await realpath(config.isolation.disposableRoot);
  if (!inside(await realpath(tmpdir()), disposable)) throw new Error('Disposable DB root must be inside the OS temporary directory.');
  const database = await realpath(config.isolation.databasePath);
  if (!inside(disposable, database)) throw new Error('Database must resolve inside disposableRoot.');
  const marker = JSON.parse(await readFile(join(disposable, '.devup-isolated.json'), 'utf8'));
  const apiOrigin = local(config.isolation.apiBaseUrl).origin; const appOrigin = local(config.appBaseUrl).origin;
  if (marker.kind !== 'devup-isolated-data-test' || !/^[a-zA-Z0-9-]{24,128}$/.test(marker.nonce ?? '') || await realpath(marker.databasePath) !== database || marker.apiOrigin !== apiOrigin || marker.appOrigin !== appOrigin) throw new Error('Disposable marker does not bind the DB and app/API origins.');
  const handshakeUrl = new URL(config.isolation.handshakePath, apiOrigin); if (handshakeUrl.origin !== apiOrigin) throw new Error('Handshake escapes isolated API.');
  const handshake = await fetch(handshakeUrl, { redirect: 'error', signal: AbortSignal.timeout(5000) }).then((response) => response.json());
  if (handshake.nonce !== marker.nonce || await realpath(handshake.databasePath) !== database) throw new Error('Isolated API handshake does not bind the disposable database.');
  const openapi = await sourceFile(project, config.openapiPath); const spec = JSON.parse(openapi.bytes);
  const sourceIdentity = await sourceDigest(project);
  const ids = new Set(); const bindings = [];
  for (const binding of config.bindings) {
    if (typeof binding.id !== 'string' || !/^[^\u0000-\u001f\u007f]{1,300}$/u.test(binding.id) || !binding.id.trim() || ids.has(binding.id)) throw new Error('Binding IDs must uniquely match the export behavior IDs.'); ids.add(binding.id);
    if (new URL(binding.api.path, apiOrigin).origin !== apiOrigin) throw new Error('API observation escapes isolated origin.');
    const api = operation(spec, binding.operationId, 'GET', binding.api.path);
    if (!binding.api.pointer?.startsWith('/') || !schemaField(spec, api.operation.responses?.['200']?.content?.['application/json']?.schema, binding.api.pointer)) throw new Error('API aggregate field is absent from the existing response schema.');
    const model = await sourceFile(project, binding.db.modelPath); const schema = JSON.parse(model.bytes);
    const fields = new Set((schema.columns ?? []).map((column) => column.name));
    if (schema.name !== binding.db.table || !fields.has(binding.db.field) || Object.keys(binding.db.where ?? {}).some((key) => !fields.has(key)) || !['count', 'sum', 'first'].includes(binding.db.aggregate)) throw new Error('DB table/field/aggregate anchors are unresolved.');
    identifier(binding.db.table); identifier(binding.db.field);
    if (!same(binding.phases?.map((phase) => phase.phase), DATA_PHASES)) throw new Error('Data phases must be baseline, mutated, empty, error in order.');
    for (const phase of binding.phases) {
      if (!phase.mutation || !['POST', 'PUT', 'PATCH', 'DELETE'].includes(phase.mutation.method)) throw new Error('Each scenario needs an explicit isolated API mutation.');
      operation(spec, phase.mutation.operationId, phase.mutation.method, phase.mutation.path);
    }
    if (!['landing', 'list', 'detail'].every((surface) => binding.views?.some((view) => view.surface === surface))) throw new Error('Every binding needs landing/list/detail observations.');
    for (const view of binding.views) if (!view.selector || !view.route?.startsWith('/') || !view.format?.includes('{value}') || !view.emptyText || !view.errorText) throw new Error('Views need a semantic selector, route, value format and explicit empty/error copy.');
    bindings.push({ ...binding, sourceAnchor: { openapiPath: openapi.path, openapiSha256: openapi.sha256, dbModelPath: model.path, dbSchemaSha256: model.sha256, matchedPath: api.path, matchedMethod: api.method } });
  }
  return { project, disposable, database, marker, apiOrigin, appOrigin, bindings, sourceDigest: sourceIdentity.sourceDigest };
}
export function aggregateQuery(db) {
  if (!['count', 'sum', 'first'].includes(db.aggregate)) throw new Error('Unsupported typed aggregate.');
  const filters = Object.entries(db.where ?? {}).sort(([a], [b]) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
  const field = identifier(db.field); const select = db.aggregate === 'count' ? `COUNT(${field})` : db.aggregate === 'sum' ? `COALESCE(SUM(${field}),0)` : field;
  const query = `SELECT ${select} AS value FROM ${identifier(db.table)}${filters.length ? ` WHERE ${filters.map(([key]) => `${identifier(key)} = ?`).join(' AND ')}` : ''}${db.aggregate === 'first' ? ` ORDER BY ${field} LIMIT 1` : ''}`;
  return { query, params: filters.map(([, value]) => typeof value === 'boolean' ? Number(value) : value) };
}
async function mutation(binding, phase, environment) {
  const target = new URL(phase.mutation.path, environment.apiOrigin); if (target.origin !== environment.apiOrigin) throw new Error('Mutation escapes isolated API.');
  const response = await fetch(target, { method: phase.mutation.method, redirect: 'error', headers: { 'content-type': 'application/json', 'x-devup-isolation': environment.marker.nonce }, body: JSON.stringify(phase.mutation.body), signal: AbortSignal.timeout(10000) });
  if (!response.ok) throw new Error(`Isolated mutation failed (${binding.id}/${phase.phase}): ${response.status}`);
  return { operationId: phase.mutation.operationId, status: response.status, isolationNonce: environment.marker.nonce };
}
export async function runDataBindingCheck(check, page, context) {
  let isolated; const receipt = { kind: 'devup-data-binding-receipt', schemaVersion: 1, planId: context.planId, planSha256: context.planSha256, bindings: [] };
  try {
    const config = check.dataBinding; const environment = await validateDataConfig(config); receipt.sourceDigest = environment.sourceDigest;
    if (context.sourceDigest && context.sourceDigest !== receipt.sourceDigest) throw new Error('Audited source digest changed before isolated data scenarios.');
    const { DatabaseSync, backup } = await import('node:sqlite');
    isolated = await page.context().browser().newContext();
    await isolated.route('**/*', (route) => [environment.apiOrigin, environment.appOrigin].includes(new URL(route.request().url()).origin) ? route.continue() : route.abort());
    const viewPage = await isolated.newPage();
    for (const binding of environment.bindings) {
      const bindingReceipt = { id: binding.id, operationId: binding.operationId, table: binding.db.table, field: binding.db.field, aggregate: binding.db.aggregate, sourceAnchor: binding.sourceAnchor, phases: [] }; receipt.bindings.push(bindingReceipt);
      try {
        for (const phase of binding.phases) {
          const mutationReceipt = await mutation(binding, phase, environment);
          const response = await fetch(new URL(binding.api.path, environment.apiOrigin), { redirect: 'error', signal: AbortSignal.timeout(10000) });
          const body = await response.json();
          const db = new DatabaseSync(environment.database, { readOnly: true, allowExtension: false });
          let rows; let snapshot;
          try {
            const { query, params } = aggregateQuery(binding.db);
            rows = db.prepare(query).all(...params).map((row) => Object.fromEntries(Object.entries(row).map(([key, value]) => [key, typeof value === 'bigint' ? Number(value) : value])));
            const directory = join(context.out, 'data', hash(binding.id).slice(0,24), String(context.viewport?.width ?? 'single')); await mkdir(directory, { recursive: true });
            const snapshotPath = join(directory, `${phase.phase}.sqlite`); await backup(db, snapshotPath);
            snapshot = { path: relative(context.out, snapshotPath).split(sep).join('/'), sha256: hash(await readFile(snapshotPath)) };
          } finally { db.close(); }
          const value = rows[0]?.value ?? null;
          const views = [];
          for (const view of binding.views) {
            const target = new URL(view.route, environment.appOrigin); if (target.origin !== environment.appOrigin) throw new Error('View route escapes isolated app.');
            await viewPage.goto(target.href, { waitUntil: 'networkidle', timeout: 15000 });
            const expectedText = phase.phase === 'error' ? view.errorText : phase.phase === 'empty' ? view.emptyText : view.format.replaceAll('{value}', String(value));
            const locator = viewPage.locator(view.selector); if (await locator.count() !== 1) throw new Error(`Ambiguous rendered binding selector: ${view.selector}`);
            await locator.waitFor({ state: 'visible', timeout: 5000 });
            await viewPage.waitForFunction(({selector,expected}) => document.querySelector(selector)?.textContent?.replace(/\s+/g, ' ').trim() === expected, { selector: view.selector, expected: normalize(expectedText) }, { timeout: 10000 }).catch(() => {});
            views.push({ surface: view.surface, route: view.route, selector: view.selector, text: normalize(await locator.textContent()), expectedText: normalize(expectedText) });
          }
          bindingReceipt.phases.push({ phase: phase.phase, mutation: mutationReceipt, api: { status: response.status, body, pointer: binding.api.pointer, observedValue: pointer(body, binding.api.pointer), expected: phase.phase === 'error' ? null : value }, db: { adapter: 'node-sqlite-readonly', ...aggregateQuery(binding.db), rows, snapshot }, views });
        }
      } finally {
        await mutation(binding, binding.phases[0], environment);
      }
    }
    const result = evaluateDataReceipt(receipt);
    const receiptPath = join(context.out, `data-binding-${hash(JSON.stringify(check)).slice(0,12)}-${context.viewport?.width ?? 'single'}.json`);
    await writeFile(receiptPath, JSON.stringify(receipt, null, 2));
    return { status: result.verified ? 'pass' : 'fail', reason: result.verified ? 'Isolated API mutations, read-only SQLite snapshots and all rendered surfaces observed.' : 'Data source/scenario evidence incomplete or inconsistent.', measured: { ...result, receipt: relative(context.out, receiptPath), receiptSha256: hash(await readFile(receiptPath)) } };
  } catch (error) { return { status: 'inconclusive', reason: String(error.message ?? error), measured: { receipt, apiVerified: false, dbVerified: false } }; }
  finally { await isolated?.close(); }
}
export function evaluateDataReceipt(receipt) {
  const bindings = (receipt.bindings ?? []).map((binding) => {
    const phases = binding.phases ?? [];
    const complete = same(phases.map((phase) => phase.phase), DATA_PHASES);
    const sourceMatched = Boolean(binding.sourceAnchor?.openapiSha256 && binding.sourceAnchor?.dbSchemaSha256);
    const dbVerified = complete && phases.every((phase) => phase.db?.adapter === 'node-sqlite-readonly' && phase.db.snapshot?.sha256 && Array.isArray(phase.db.rows));
    const apiVerified = complete && phases.every((phase) => phase.phase === 'error' ? phase.api?.status >= 400 : phase.api?.status === 200 && same(pointer(phase.api.body, phase.api.pointer), phase.db?.rows?.[0]?.value ?? null));
    const changed = complete && !same(phases[0].db?.rows, phases[1].db?.rows);
    const empty = complete && (['count', 'sum'].includes(binding.aggregate) ? phases[2].db?.rows?.[0]?.value === 0 : phases[2].db?.rows?.length === 0);
    const rendered = complete && phases.every((phase) => ['landing', 'list', 'detail'].every((surface) => phase.views?.some((view) => view.surface === surface && view.text === view.expectedText)));
    return { id: binding.id, sourceMatched, apiVerified, dbVerified, phases: Object.fromEntries(DATA_PHASES.map((name) => [name, phases.some((phase) => phase.phase === name)])), changed, empty, rendered, verified: sourceMatched && dbVerified && apiVerified && changed && empty && rendered };
  });
  return { bindings, verified: bindings.length > 0 && bindings.every((binding) => binding.verified), apiVerified: bindings.length > 0 && bindings.every((binding) => binding.apiVerified), dbVerified: bindings.length > 0 && bindings.every((binding) => binding.dbVerified), evidenceLimit: 'SQLite query and browser observations are executor-observed; ingest independently hashes the plan, anchors and snapshot files.' };
}
