import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { appendFileSync } from 'node:fs';
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
