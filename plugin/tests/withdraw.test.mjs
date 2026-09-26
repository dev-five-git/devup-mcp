// The committed bundle, `dist/code.js`, answering `devup-cancel`: devup-mcp
// sends it when nobody waits for a job any more - the process that asked went
// away, or the read timed out. A job still waiting its turn is not run, and
// one already running is not answered.

import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import vm from 'node:vm'

const bundle = await readFile(new URL('../dist/code.js', import.meta.url), 'utf8')

/** The plugin's main thread against a Figma that records what it is asked. */
function plugin() {
  const posted = []
  const looked = []
  const figma = {
    showUI() {},
    on() {},
    ui: { postMessage: (message) => posted.push(message), onmessage: null },
    currentPage: { id: '0:1', name: 'Page 1', selection: [] },
    root: { name: 'Landing' },
    fileKey: null,
    // The first thing the metadata script does. Nothing is found, so every
    // job that runs answers with an error - which still is an answer.
    getNodeByIdAsync: async (id) => {
      looked.push(id)
      return null
    },
  }
  vm.runInNewContext(bundle, { figma, __html__: '', console })
  return { posted, looked, send: (message) => figma.ui.onmessage(message) }
}

const job = (requestId, nodeId) => ({
  kind: 'devup-job',
  requestId,
  script: 'metadata',
  params: { nodeId },
})

const cancel = (requestId) => ({ kind: 'devup-cancel', requestId })

/** Every job the messages started has finished. Nothing here waits on a timer. */
const settled = () => new Promise((resolve) => setImmediate(resolve))

test('a job withdrawn while it waits its turn is never run', async () => {
  const { posted, looked, send } = plugin()
  send(job('a', '1:1'))
  send(job('b', '2:2'))
  send(cancel('b'))
  await settled()
  assert.deepEqual(looked, ['1:1'])
  assert.deepEqual(
    posted.map((message) => message.requestId),
    ['a'],
  )
})

test('a job withdrawn while it runs is not answered, and the queue goes on', async () => {
  const { posted, looked, send } = plugin()
  send(job('a', '1:1'))
  send(cancel('a'))
  send(job('b', '2:2'))
  await settled()
  assert.deepEqual(looked, ['1:1', '2:2'])
  assert.deepEqual(
    posted.map((message) => message.requestId),
    ['b'],
  )
})

test('withdrawing a job that already answered changes nothing', async () => {
  const { posted, send } = plugin()
  send(job('a', '1:1'))
  await settled()
  send(cancel('a'))
  send(job('a2', '1:1'))
  await settled()
  assert.deepEqual(
    posted.map((message) => message.requestId),
    ['a', 'a2'],
  )
})
