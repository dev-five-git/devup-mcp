// The committed bundle, `dist/code.js`, swapping its scripts when devup-mcp
// sends newer ones (`devup-scripts-update`), so a rebuilt server reaches an
// open plugin without the plugin being closed and run again.

import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import vm from 'node:vm'

const bundle = await readFile(new URL('../dist/code.js', import.meta.url), 'utf8')
const shipped = await readFile(
  new URL('../../crates/devup-mcp-figma/src/plugin_scripts_bundle.js', import.meta.url),
  'utf8',
)

function plugin() {
  const posted = []
  const figma = {
    showUI() {},
    on() {},
    ui: { postMessage: (message) => posted.push(message), onmessage: null },
    currentPage: { id: '0:1', name: 'Page 1', selection: [] },
    root: { name: 'Landing' },
    fileKey: null,
    getNodeByIdAsync: async () => null,
  }
  vm.runInNewContext(bundle, { figma, __html__: '', console, setTimeout, clearTimeout })
  return { posted, send: (message) => figma.ui.onmessage(message) }
}

// Messages are built inside the vm context, whose Object.prototype differs from
// this realm's, so compare plain copies.
const plain = (value) => JSON.parse(JSON.stringify(value))
const settled = () => new Promise((resolve) => setImmediate(resolve))
const job = (requestId, script) => ({ kind: 'devup-job', requestId, script, params: { nodeId: '1:1' } })
const update = (scriptsHash, source) => ({ kind: 'devup-scripts-update', scriptsHash, bundle: source })

test('scripts sent by devup-mcp replace the built-in ones for later jobs', async () => {
  const { posted, send } = plugin()
  send(update('new-hash', '(function () { return { metadata: async () => ({ swapped: true }) } })'))
  send(job('a', 'metadata'))
  await settled()
  assert.deepEqual(plain(posted.find((m) => m.kind === 'devup-scripts-updated')), {
    kind: 'devup-scripts-updated',
    scriptsHash: 'new-hash',
  })
  assert.deepEqual(plain(posted.find((m) => m.requestId === 'a').data), { swapped: true })
})

test('a bundle that does not load leaves the current scripts in place', async () => {
  const { posted, send } = plugin()
  for (const source of ['not javascript (', '42', '(function () { return { metadata: 1 } })']) {
    send(update('bad', source))
  }
  send(job('a', 'metadata'))
  await settled()
  const reports = posted.filter((m) => m.kind === 'devup-scripts-updated')
  assert.equal(reports.length, 3)
  assert.ok(reports.every((m) => typeof m.error === 'string'))
  // The built-in metadata script ran: it looked the node up and found nothing.
  assert.match(posted.find((m) => m.requestId === 'a').error, /DEVUP_/)
})

test('the bundle devup-mcp ships loads in the plugin and names every script', async () => {
  const { posted, send } = plugin()
  send(update('shipped', shipped))
  send(job('ctx', 'bridgeContext'))
  await settled()
  assert.deepEqual(plain(posted.find((m) => m.kind === 'devup-scripts-updated')), {
    kind: 'devup-scripts-updated',
    scriptsHash: 'shipped',
  })
  assert.equal(posted.find((m) => m.requestId === 'ctx').data.kind, 'devup-bridge-context')
})
