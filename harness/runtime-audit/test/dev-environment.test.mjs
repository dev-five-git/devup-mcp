import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { appendFile, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { reoptimizationReload, serverLogReader } from '../dev-environment.mjs';
import { runPlan } from '../run.mjs';

test('only optimizer messages announcing reload invalidate measurements', () => {
  assert.ok(reoptimizationReload('[vite] new dependencies optimized, reloading'));
  assert.ok(reoptimizationReload('\x1b[32moptimized dependencies changed. reloading\x1b[0m'));
  assert.equal(reoptimizationReload('[vite] new dependencies optimized: react'), false);
  assert.equal(reoptimizationReload('[vite] connected.'), false);
});

test('server logs ignore historical events and retain split lines and truncation', async () => {
  const out = await mkdtemp(join(tmpdir(), 'devup-server-log-'));
  try {
    const path = join(out, 'server.log');
    await writeFile(path, 'new dependencies optimized, reloading\n');
    const read = await serverLogReader(path);
    assert.deepEqual(await read(), []);
    await appendFile(path, 'optimized dependencies changed. ');
    assert.deepEqual(await read(), []);
    await appendFile(path, 'reloading\n');
    assert.equal((await read())[0].kind, 'dev-reoptimization-reload');
    assert.deepEqual(await read(), []);
    await writeFile(path, 'new dependencies optimized, reloading\n');
    assert.equal((await read()).length, 1);
  } finally { await rm(out, { recursive: true, force: true }); }
});

test('PowerShell UTF-16 logs identify SSR reloads and optimizer reloads', async () => {
  const out = await mkdtemp(join(tmpdir(), 'devup-server-log-windows-'));
  try {
    const path = join(out, 'server.log');
    await writeFile(path, '\uFEFFserver ready\r\n', 'utf16le');
    const read = await serverLogReader(path);
    await appendFile(path, '[vite] (ssr) program reload\r\n', 'utf16le');
    assert.equal((await read())[0].kind, 'dev-server-reload');
    await appendFile(path, '[vite] optimized dependencies changed. reloading\r\n', 'utf16le');
    assert.equal((await read())[0].kind, 'dev-reoptimization-reload');
  } finally { await rm(out, { recursive: true, force: true }); }
});

for (const source of ['console', 'server-log', 'ssr-reload', 'websocket', 'ordinary-page-error']) {
  test(`${source}: reloads are environmental and a stable subsequent route is still audited`, async () => {
    const out = await mkdtemp(join(tmpdir(), 'devup-dev-reload-'));
    const log = join(out, 'server.log');
    await writeFile(log, '');
    const server = createServer(async (request, response) => {
      const unstable = request.url === '/unstable';
      if (unstable && source === 'server-log') await appendFile(log, '[vite] new dependencies optimized, reloading\n');
      if (unstable && source === 'ssr-reload') await appendFile(log, '[vite] (ssr) program reload\n');
      response.setHeader('Content-Type', 'text/html; charset=utf-8');
      response.end(`<body><h1>${unstable ? 'This page couldn’t load' : 'Healthy page'}</h1><script>
        ${unstable && source === 'console' ? "console.info('[vite] new dependencies optimized, reloading');" : ''}
        ${unstable && source === 'websocket' ? "new WebSocket('ws://' + location.host);" : ''}
        setTimeout(() => document.body.dataset.ready = 'true', 100);
      </script></body>`);
    });
    const sockets = new Set();
    server.on('upgrade', (request, socket) => {
      sockets.add(socket);
      const accept = createHash('sha1').update(request.headers['sec-websocket-key'] + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
      socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
      const frame = Buffer.from(JSON.stringify({ type: 'full-reload', path: '*' }));
      socket.write(Buffer.concat([Buffer.from([0x81, frame.length]), frame]));
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const baseUrl = `http://127.0.0.1:${server.address().port}`;
    try {
      const plan = join(out, 'plan.json');
      await writeFile(plan, JSON.stringify({ schemaVersion: 1, kind: 'devup-runtime-audit-plan', planId: 'dev-reload', plan: {
        baseUrl, allowedOrigins: [baseUrl], viewports: [{ width: 390, height: 200 }],
        readiness: { selector: 'body[data-ready="true"]' }, screens: ['/unstable', '/healthy'].map((routePath) => ({
          id: routePath, routePath, checks: [{ kind: 'page-health' }, { kind: 'no-horizontal-overflow' }],
        })),
      } }));
      const report = await runPlan({ plan, out, maxMinutes: 1, serverLog: log });
      const [unstable, healthy] = report.runs[0].screens;
      if (source === 'ordinary-page-error') {
        assert.equal(unstable.checks[0].status, 'fail', 'real page errors remain failures');
        assert.deepEqual(report.runs[0].environmentErrors, []);
      } else {
        assert.equal(report.status, 'partial');
        assert.ok(report.runs[0].environmentErrors.some((e) => e.kind === (['websocket', 'ssr-reload'].includes(source) ? 'dev-server-reload' : 'dev-reoptimization-reload')));
        assert.equal(unstable.status, 'skipped');
        assert.deepEqual(unstable.checks.map((c) => c.status), ['inconclusive', 'inconclusive']);
        assert.equal(unstable.screenshot, undefined);
      }
      assert.equal(healthy.status, 'completed');
      assert.deepEqual(healthy.checks.map((c) => c.status), ['pass', 'pass']);
    } finally {
      for (const socket of sockets) socket.destroy();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
      await rm(out, { recursive: true, force: true });
    }
  });
}
