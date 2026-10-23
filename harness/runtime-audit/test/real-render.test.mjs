import assert from 'node:assert/strict';
import { execFile, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFile, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import test from 'node:test';
import { build, preview } from 'vite';
import { DevupUI } from '@devup-ui/vite-plugin';
import { verifyRender } from '../verify-render.mjs';

const harness = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repo = resolve(harness, '../..');
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const execute = promisify(execFile);

async function cli(args) {
  // Keep the Vite preview server's event loop free while Chromium visits it.
  try {
    return { status: 0, ...await execute(process.execPath, [join(harness, 'verify-render.mjs'), ...args], { windowsHide: true, timeout: 60_000 }) };
  } catch (error) {
    return { status: error.code, stdout: error.stdout, stderr: error.stderr };
  }
}

function cargo(args) {
  const result = spawnSync('cargo', args, { cwd: repo, encoding: 'utf8', windowsHide: true, timeout: 300_000 });
  assert.equal(result.status, 0, `${args.join(' ')}\n${result.error ?? ''}\n${result.stderr}`);
  return result.stdout;
}

test('production codegen renders in Chromium; geometry alone cannot pass broken pixels or assets', { timeout: 360_000 }, async () => {
  const outRoot = join(harness, 'out');
  await mkdir(outRoot, { recursive: true });
  const dir = await mkdtemp(join(outRoot, 'real-render-'));
  let server;
  try {
    cargo(['run', '--quiet', '-p', 'devup-mcp-devup-ui', '--example', 'render_fixture', '--',
      join(harness, 'fixtures/render-snapshot.json'), join(dir, 'Generated.tsx')]);
    cargo(['build', '--quiet', '-p', 'devup-mcp-visual']);
    const target = JSON.parse(cargo(['metadata', '--format-version', '1', '--no-deps'])).target_directory;
    const visualBin = join(target, 'debug', process.platform === 'win32' ? 'devup-mcp-visual.exe' : 'devup-mcp-visual');
    await mkdir(join(dir, 'public/images'), { recursive: true });
    const hero = await readFile(join(harness, 'fixtures/render-hero.png'));
    await writeFile(join(dir, 'public/images/hero.png'), hero);
    await writeFile(join(dir, 'devup.json'), JSON.stringify({ theme: {} }));
    await writeFile(join(dir, 'index.html'), '<html><body style="margin:0"><div id="root"></div><script type="module" src="/main.tsx"></script></body></html>');
    await writeFile(join(dir, 'main.tsx'), `import '@devup-ui/reset-css';
import { createRoot } from 'react-dom/client';
import { flushSync } from 'react-dom';
import { RenderFixture } from './Generated';
flushSync(() => createRoot(document.getElementById('root')!).render(<RenderFixture />));
const mode = new URLSearchParams(location.search).get('mode');
const style = document.createElement('style');
style.textContent = mode === 'missing-background' ? '[data-figma-node-id="1:1"] img{visibility:hidden!important}'
 : mode === 'wrong-color' ? '[data-figma-node-id="1:2"]{background:#00ff00!important}'
 : '';
if (mode === 'unloaded-asset') document.querySelector('img')!.src = '/missing.png';
document.head.append(style);`);
    const config = { root: dir, configFile: false, logLevel: 'error', plugins: [DevupUI()], build: { outDir: 'dist' }, preview: { host: '127.0.0.1', port: 0 } };
    await build(config);
    server = await preview(config);
    const baseUrl = `http://127.0.0.1:${server.httpServer.address().port}`;
    const reference = join(harness, 'fixtures/render-reference.png');
    const references = join(dir, 'references.json');
    await writeFile(references, JSON.stringify({ references: [{ screen: 'hero', width: 320, height: 240,
      path: reference, sha256: sha256(await readFile(reference)), threshold: 0.005 }] }));
    for (const mode of ['correct', 'missing-background', 'wrong-color', 'unloaded-asset']) {
      const plan = join(dir, `${mode}.json`);
      await writeFile(plan, JSON.stringify({ schemaVersion: 1, kind: 'devup-runtime-audit-plan', planId: mode, plan: {
        baseUrl, allowedOrigins: [baseUrl], viewports: [{ width: 320, height: 240, deviceScaleFactor: 1 }],
        readiness: { selector: '[data-figma-node-id="1:2"]' },
        screens: [{ id: 'hero', routePath: `/?mode=${mode}`, checks: [
          { kind: 'bounds', subject: { css: '[data-figma-node-id="1:1"]' }, container: { css: 'body' }, expected: { x: 0, y: 0, width: 320, height: 240 } },
          { kind: 'image-loaded' }, { kind: 'page-health' },
        ] }],
      } }));
      const output = join(dir, mode);
      const command = await cli(['--plan', plan, '--references', references, '--out', output, '--visual-bin', visualBin]);
      assert.equal(command.status, mode === 'correct' ? 0 : 1, `${mode}: ${command.stderr}\n${command.stdout}`);
      const result = JSON.parse(await readFile(join(output, 'real-render.json'), 'utf8'));
      const runtime = JSON.parse(await readFile(join(output, 'report.json'), 'utf8'));
      assert.equal(runtime.runs[0].screens[0].checks[0].status, 'pass', JSON.stringify(runtime));
      assert.equal(result.verdict, mode === 'correct' ? 'pass' : 'fail', JSON.stringify(result));
      assert.equal(result.comparisons[0].status, mode === 'correct' ? 'exact' : 'mismatch', JSON.stringify(result));
      assert.ok((await readFile(join(output, 'visual-diff-0.png'))).length > 0);
      const images = runtime.runs[0].screens[0].checks[1];
      assert.equal(images.status, mode === 'unloaded-asset' ? 'fail' : 'pass');
    }
    const plan = join(dir, 'correct.json');
    const valid = JSON.parse(await readFile(references, 'utf8'));
    await writeFile(references, JSON.stringify({ references: [valid.references[0], valid.references[0]] }));
    await assert.rejects(verifyRender({ plan, references, out: join(dir, 'duplicate-reference'), visualBin }), /one Figma reference/);
    valid.references[0].sha256 = '0'.repeat(64);
    await writeFile(references, JSON.stringify(valid));
    await assert.rejects(verifyRender({ plan, references, out: join(dir, 'changed-reference'), visualBin }), /hash differs/);
    await writeFile(references, '{"references":[]}');
    await assert.rejects(verifyRender({ plan, references, out: join(dir, 'missing-reference'), visualBin }), /manifest is empty/);
    const invalid = await cli(['--plan', plan, '--references', references, '--out', join(dir, 'invalid'), '--visual-bin', visualBin]);
    assert.equal(invalid.status, 2, invalid.stderr);
  } finally {
    if (server) await new Promise((done) => server.httpServer.close(done));
    // Preserve browser evidence on request for local diagnosis and CI uploads.
    if (!process.env.DEVUP_KEEP_RENDER_EVIDENCE) await rm(dir, { recursive: true, force: true });
  }
});
