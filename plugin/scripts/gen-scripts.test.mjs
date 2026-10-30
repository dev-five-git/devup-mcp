import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { appendFileSync, readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const generator = fileURLToPath(new URL('./gen-scripts.mjs', import.meta.url));
const moduleUrl = new URL('../src/generated/scripts.js', import.meta.url);
const generate = async () => {
  const child = spawn(process.execPath, [generator], { stdio: ['ignore', 'ignore', 'pipe'] });
  let error = '';
  child.stderr.on('data', (data) => { error += data; });
  const [code] = await once(child, 'exit');
  assert.equal(code, 0, error);
};

test('script identity hashes the generated module and matches the Rust identity', async () => {
  await generate();
  const expected = createHash('sha256').update(readFileSync(moduleUrl)).digest('hex');
  const identity = JSON.parse(readFileSync(new URL('../src/generated/scripts-hash.json', import.meta.url), 'utf8'));
  assert.equal(identity.scriptsHash, expected);
  assert.equal(identity.protocol, 1);
  const rust = readFileSync(new URL('../../crates/devup-mcp-figma/src/plugin_scripts.rs', import.meta.url), 'utf8');
  assert.ok(rust.replace(/\s+/g, ' ').includes(`pub const PLUGIN_SCRIPTS_HASH: &str = "${expected}";`));
});

test('concurrent generation always publishes a complete importable module', async () => {
  await generate();
  appendFileSync(moduleUrl, '\n// Force concurrent republishing of an otherwise complete module.\n');
  const outcomes = await Promise.allSettled([
    ...Array.from({ length: 6 }, generate),
    (async () => {
      for (let index = 0; index < 40; index += 1) {
        const { SCRIPTS } = await import(`${moduleUrl.href}?read=${index}`);
        assert.equal(typeof SCRIPTS?.assets, 'function');
        await new Promise((resolve) => setTimeout(resolve, 2));
      }
    })(),
  ]);
  for (const result of outcomes) if (result.status === 'rejected') throw result.reason;
});
