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
  const context = { figma, __html__: '', console, setTimeout, clearTimeout }
  vm.runInNewContext(bundle, context)
  return { posted, looked, context, send: (message) => figma.ui.onmessage(message) }
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

// The image conversion the reference PNG script hands to the UI waits only as
// long as the script gives it: an answer that never comes is nothing.
test('an image conversion the UI never answers is given up at its time', async () => {
  const { posted, context } = plugin()
  const started = Date.now()
  const png = await context.__devupNormalizeImage(new Uint8Array(4), 8192, 30, 2, 1)
  assert.equal(png, null)
  assert.ok(Date.now() - started < 5000)
  const asked = posted.find((message) => message.kind === 'devup-decode')
  assert.deepEqual([asked.maxEdge, asked.width, asked.height], [8192, 2, 1])
})

// Fixed measurement source and committed plugin acceptance stay in this suite
// so the existing CI entry point exercises them before dependency installation.
const measureSource=await readFile(new URL('../../crates/devup-mcp-figma/src/scripts/measure_nodes.js',import.meta.url),'utf8')
const measureNodes=({rootIds})=>new (Object.getPrototypeOf(async function(){}).constructor)('figma','rootIds',measureSource.replace('"__DEVUP_ROOT_IDS__"','rootIds'))(globalThis.figma,rootIds)

test('measurement reads only requested layout fields, supports instances and discloses unavailable nodes', async () => {
  const looked=[]
  const node=new Proxy({type:'FRAME',width:167,height:94,layoutMode:'HORIZONTAL',itemSpacing:8,paddingTop:1,paddingRight:2,paddingBottom:3,paddingLeft:4,primaryAxisAlignItems:'CENTER',counterAxisAlignItems:'MIN'}, {
    get(target,key) {if (key === "then") return undefined; assert.ok(key in target, `unexpected heavy read ${String(key)}`);return target[key]}
  })
  globalThis.figma={getNodeByIdAsync:async id=>{looked.push(id);return id==='2:2'?null:node}}
  const result=await measureNodes({rootIds:['I1:2;3:4','2:2']})
  assert.deepEqual(looked,['I1:2;3:4','2:2'])
  assert.equal(result.nodes[0].width,167)
  assert.deepEqual(result.nodes[0].padding,{top:1,right:2,bottom:3,left:4})
  assert.equal(result.nodes[1].status,'unavailable')
  assert.equal(result.partial,true)
  assert.equal(result.coordinateSpace,'figma-logical')
})

test('fixed measurement script refuses execution strings, malformed IDs, duplicates and excess batch before reads', async () => {
  globalThis.figma={getNodeByIdAsync:()=>assert.fail('invalid IDs must never reach Figma')}
  for (const rootIds of [[],['1:2;3:4'],['1:2;alert(1)'],['1:2','1:2'],Array.from({length:11},(_,i)=>`1:${i}`)]) {
    await assert.rejects(measureNodes({rootIds}),/DEVUP_INVALID_INPUT/)
  }
})

test('the compiled plugin refreshes page and selection without an event, retaining history only as diagnostics', async () => {
  const bundle=await readFile(new URL('../dist/code.js',import.meta.url),'utf8')
  const posted=[]
  const figma={showUI(){},on(){},ui:{postMessage:m=>posted.push(m)},root:{name:'Synthetic'},fileKey:null,
    currentPage:{id:'0:1',name:'First',type:'PAGE',selection:[{id:'1:1',name:'Old',type:'FRAME'}]},viewport:{zoom:0.83}}
  vm.runInNewContext(bundle,{figma,__html__:'',console,setTimeout,clearTimeout})
  figma.ui.onmessage({kind:'devup-ready'})
  figma.currentPage={id:'0:2',name:'Current',type:'PAGE',selection:[{id:'2:2',name:'New',type:'FRAME'}]}
  figma.ui.onmessage({kind:'devup-job',requestId:'fresh',script:'bridgeContext',params:{}})
  await new Promise(resolve=>setImmediate(resolve))
  const result=posted.find(m=>m.requestId==='fresh').data
  assert.equal(result.currentPage.id,'0:2')
  assert.equal(result.selection[0].id,'2:2')
  assert.equal(result.zoom,0.83)
  assert.equal(result.uiDevicePixelRatio,undefined)
  figma.currentPage.selection=[]
  figma.ui.onmessage({kind:'devup-job',requestId:'empty',script:'bridgeContext',params:{}})
  await new Promise(resolve=>setImmediate(resolve))
  const empty=posted.find(m=>m.requestId==='empty').data
  assert.equal(empty.selectionCount,0)
  assert.equal(empty.lastKnownSelection[0].id,'2:2')
  assert.equal(empty.lastKnownPage.id,'0:2')
})

test('maximal bounded measurement batch stays below the direct text ceiling', async () => {
  const node={type:'FRAME',width:1e12,height:1e12,paddingTop:-1e12,paddingRight:1e12,paddingBottom:1e12,paddingLeft:1e12,itemSpacing:-1e12,layoutMode:'A'.repeat(32),primaryAxisAlignItems:'A'.repeat(32),counterAxisAlignItems:'A'.repeat(32)}
  globalThis.figma={getNodeByIdAsync:async()=>node}
  const rootIds=Array.from({length:10},(_,i)=>`I${'9'.repeat(50)}:1;${'8'.repeat(50)}:${i}`)
  const result=await measureNodes({rootIds})
  assert.equal(result.nodes.length,10)
  assert.ok(Buffer.byteLength(JSON.stringify(result),'utf8')<19000)
})
