// 생성된 스크립트를 실제로 불러와 실행해 본다.
//
// 타입체크와 번들만으로는 코드젠이 옳았는지 알 수 없다. 최상위 `return` 이 함수
// 밖에 놓이거나 플레이스홀더가 이상한 값으로 바뀌면 번들은 그대로 나오고 Figma
// 안에서야 터진다. 그때는 원인이 플러그인인지 스크립트인지 가리기 어렵다.
//
// 그래서 여기서 한 번 불러오고(구조가 깨졌으면 import 가 실패한다), 최소한의
// figma 스텁으로 실행까지 해 본다.

import assert from 'node:assert/strict'

const { SCRIPTS, pageCatalog, referencePng, referencePngChunk, usedResources } = await import(
  '../src/generated/scripts.js'
)

/** Rust 의 `BuiltinScript::plugin_name` 이 내놓는 이름. 어긋나면 런타임에야 안다. */
const EXPECTED = [
  'assets',
  'explore',
  'fastSnapshot',
  'fastTheme',
  'largeValue',
  'metadata',
  'pageCatalog',
  'referencePng',
  'referencePngChunk',
  'search',
  'sectionIndex',
  'snapshot',
  'usedResources',
  'variableCatalog',
  'variables',
]

const actual = Object.keys(SCRIPTS).sort()
assert.deepEqual(
  actual,
  [...EXPECTED].sort(),
  'SCRIPTS 의 이름이 Rust 의 plugin_name() 과 달라졌습니다. 두 곳을 함께 고쳐야 합니다.',
)

for (const name of EXPECTED) {
  assert.equal(
    typeof SCRIPTS[name],
    'function',
    `${name} 이 함수가 아닙니다 — 코드젠 래핑이 깨졌습니다`,
  )
}

// 실행까지 확인한다. `pageCatalog` 는 figma 표면을 가장 적게 쓰므로 스텁이
// 스크립트를 대신 검증해 버리는 일이 없다.
globalThis.figma = {
  fileKey: 'FileKey123',
  root: {
    children: [
      { id: '0:1', type: 'PAGE', name: 'Page 1' },
      { id: '0:2', type: 'PAGE', name: 'Page 2' },
    ],
  },
}

const catalog = await pageCatalog({})
assert.equal(catalog.fileKey, 'FileKey123')
assert.deepEqual(catalog.rootIds, ['0:1', '0:2'])
assert.equal(catalog.nodes.length, 2)
assert.equal(catalog.nodes[0].fields.name, 'Page 1')
assert.equal(catalog.nodes[1].type, 'PAGE')

// 참조 PNG 는 수집기가 공식 `get_screenshot` 과 같은 그림으로 받으므로, 무엇을
// export 하는지 — 다른 레이어가 닿지 않으면 캔버스, 닿으면 노드만과 조상들의 설명 —
// 와 언제 그것을 접는지가 계약이다. 거절하는 경우는 없다.
const exports = []
const solid = (value, opacity = 1) => ({
  type: 'SOLID', visible: true, opacity, blendMode: 'NORMAL', color: { r: value / 255, g: value / 255, b: value / 255 },
})
const box = (x, y, width, height) => ({ x, y, width, height })
const page = { type: 'PAGE', backgrounds: [solid(30)], children: [] }
const nest = (parent, child) => {
  child.parent = parent
  parent.children = [...(parent.children ?? []), child]
  return child
}
// 내보내는 PNG 의 머리말에 크기를 싣는다(가로 4, 세로 2).
const png = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 2])
const node = (fields) => {
  const made = {
    type: 'FRAME', name: 'n', visible: true, opacity: 1, blendMode: 'PASS_THROUGH', fills: [], strokes: [], effects: [],
    children: [], topLeftRadius: 0, topRightRadius: 0, bottomRightRadius: 0, bottomLeftRadius: 0,
    exportAsync: async (settings) => {
      exports.push({ id: made.id, ...settings })
      return png
    },
    findOne(test) {
      for (const child of this.children) {
        if (test(child)) return child
        const deeper = child.findOne && child.findOne(test)
        if (deeper) return deeper
      }
      return null
    },
    ...fields,
  }
  const at = fields.absoluteBoundingBox
  made.absoluteRenderBounds = fields.absoluteRenderBounds ?? at
  made.absoluteTransform = [[1, 0, at.x], [0, 1, at.y]]
  made.width = at.width
  made.height = at.height
  return made
}
const gradient = {
  type: 'GRADIENT_LINEAR', visible: true, opacity: 1, blendMode: 'NORMAL', gradientTransform: [[1, 0, 0], [0, 1, 0]],
  gradientStops: [{ position: 0, color: { r: 1, g: 1, b: 1, a: 1 } }, { position: 1, color: { r: 0, g: 0, b: 0, a: 1 } }],
}
const section = nest(page, node({ id: 's', type: 'SECTION', fills: [solid(255, 0.16)], absoluteBoundingBox: box(0, 0, 4000, 12000) }))
const screen = nest(section, node({ id: '1:2', absoluteBoundingBox: box(100, 100, 360, 740), absoluteRenderBounds: box(96, 100, 368, 748) }))
const tall = nest(section, node({ id: '1:3', absoluteBoundingBox: box(500, 100, 1920, 9000) }))
const card = nest(section, node({ id: 'card', fills: [gradient], clipsContent: true, absoluteBoundingBox: box(3000, 0, 400, 400) }))
const inCard = nest(card, node({ id: '1:4', absoluteBoundingBox: box(3010, 10, 100, 100) }))
nest(card, node({ id: 'cap', type: 'TEXT', name: 'Caption', absoluteBoundingBox: box(3050, 50, 300, 40) }))
nest(section, node({ id: 'under', type: 'TEXT', name: 'Under', absoluteBoundingBox: box(5010, 10, 50, 50) }))
const plate = nest(section, node({ id: 'plate', fills: [solid(200)], absoluteBoundingBox: box(5000, 0, 400, 400) }))
const onPlate = nest(plate, node({ id: '1:5', absoluteBoundingBox: box(5000, 0, 100, 100) }))
const inset = nest(section, node({ id: 'inset', fills: [solid(200)], effects: [{ type: 'NOISE', visible: true }], absoluteBoundingBox: box(6000, 0, 400, 400) }))
const inInset = nest(inset, node({ id: '1:6', absoluteBoundingBox: box(6010, 10, 100, 100) }))
nest(inset, node({ id: 'over', type: 'TEXT', name: '덮는 글', absoluteBoundingBox: box(6050, 50, 100, 40) }))
const shaky = nest(card, node({ id: '1:9', absoluteBoundingBox: box(3200, 10, 100, 100) }))
const exportShaky = shaky.exportAsync
shaky.exportAsync = async (settings) => {
  if (settings.contentsOnly) throw new Error('Unable to establish connection to Figma after 10 seconds.')
  return exportShaky(settings)
}
const glassHost = nest(section, node({ id: 'glassHost', fills: [solid(68)], absoluteBoundingBox: box(7000, 0, 400, 400) }))
nest(glassHost, node({ id: 'blob', type: 'TEXT', name: 'Blob', absoluteBoundingBox: box(7100, 100, 100, 100) }))
const glass = nest(glassHost, node({
  id: '1:7', fills: [solid(255, 0.2)], effects: [{ type: 'BACKGROUND_BLUR', visible: true, radius: 10 }],
  absoluteBoundingBox: box(7000, 0, 240, 240),
}))
nest(glass, node({ id: 'label', type: 'TEXT', name: 'Label', absoluteBoundingBox: box(7010, 10, 50, 20) }))
const maskHost = nest(section, node({ id: 'maskHost', fills: [gradient], absoluteBoundingBox: box(8000, 0, 400, 400) }))
const maskLayer = nest(maskHost, node({ id: 'mask', isMask: true, maskType: 'ALPHA', absoluteBoundingBox: box(8000, 0, 100, 100), absoluteRenderBounds: null }))
const maskShape = nest(maskLayer, node({ id: '1:11', type: 'VECTOR', fills: [solid(0)], absoluteBoundingBox: box(8000, 0, 100, 100) }))
const maskedNode = nest(maskHost, node({ id: '1:8', absoluteBoundingBox: box(8000, 0, 100, 100) }))
nest(maskHost, node({ id: 'cover', type: 'TEXT', name: 'Cover', absoluteBoundingBox: box(8050, 50, 100, 40) }))
const photo = nest(section, node({
  id: 'photo', fills: [{ type: 'IMAGE', visible: true, opacity: 1, blendMode: 'NORMAL', imageHash: 'big', scaleMode: 'FILL' }],
  absoluteBoundingBox: box(9000, 0, 400, 400),
}))
const onPhoto = nest(photo, node({ id: '1:10', absoluteBoundingBox: box(9010, 10, 100, 100) }))
nest(photo, node({ id: 'caption2', type: 'TEXT', name: 'Caption2', absoluteBoundingBox: box(9050, 50, 100, 40) }))
const nodes = Object.fromEntries([screen, tall, inCard, onPlate, inInset, glass, maskedNode, shaky, onPhoto, maskShape].map((n) => [n.id, n]))
globalThis.figma = {
  mixed: Symbol('mixed'),
  getNodeByIdAsync: async (id) => nodes[id] ?? null,
  getImageByHash: () => null,
  base64Encode: (bytes) => Buffer.from(bytes).toString('base64'),
}
const run = async (id) => {
  exports.length = 0
  return referencePng({ nodeId: id })
}
// 닿는 레이어가 없으면 캔버스가 그 그림이다 — 페이지 배경만 알린다.
const alone = await run('1:2')
assert.deepEqual(exports, [{ id: '1:2', format: 'PNG', constraint: { type: 'SCALE', value: 1 }, contentsOnly: false, useAbsoluteBounds: false, colorProfile: 'SRGB' }])
assert.deepEqual(alone, { kind: 'devupReferencePng', nodeId: '1:2', mimeType: 'image/png', byteLength: 24, data: Buffer.from(png).toString('base64'), backdrop: [30, 30, 30] })
await run('1:3')
assert.deepEqual(exports[0].constraint, { type: 'HEIGHT', value: 8192 })
// 그라디언트 위에 다른 레이어가 닿으면 노드만 export 하고 조상을 설명한다. 캔버스도 함께 실린다.
const composed = await run('1:4')
assert.deepEqual(exports.map((e) => [e.id, e.contentsOnly]), [['1:4', true], ['1:4', false]])
assert.equal(composed.compose.layers.length, 2)
assert.equal(composed.compose.layers[1].fills[0].type, 'GRADIENT_LINEAR')
assert.deepEqual(composed.compose.node.picture.area, box(3010, 10, 100, 100))
assert.deepEqual([composed.compose.width, composed.compose.height], [4, 2])
assert.match(composed.reaching, /TEXT "Caption"/)
assert.equal(composed.approximate, undefined)
// 아래에 깔린 레이어가 불투명한 단색 아래 숨으면 닿는 것으로 치지 않는다.
const hidden = await run('1:5')
assert.equal(hidden.compose, undefined)
// 그리지 못하는 효과가 있으면 캔버스를 쓰고 이유를 단다.
const inner = await run('1:6')
assert.match(inner.approximate, /NOISE/)
assert.match(inner.note, /may show here: TEXT "덮는 글"/)
// 노드만의 export 가 실패해도 거절하지 않는다 — 캔버스 그림에 그 이유를 단다.
const shaken = await run('1:9')
assert.equal(shaken.compose, undefined)
assert.equal(shaken.data, Buffer.from(png).toString('base64'))
assert.match(shaken.approximate, /could not be exported: Unable to establish connection to Figma/)
// 배경 흐림이 아래 레이어를 머금으면 노드를 제 칠과 자식들의 export 로 그린다.
const parts = await run('1:7')
assert.ok(parts.compose.node.own, JSON.stringify(parts))
assert.deepEqual(parts.compose.node.children.map((c) => c.area), [box(7010, 10, 50, 20)])
assert.equal(parts.compose.node.picture, undefined)
// 마스크가 노드를 자르면 그 마스크의 export 가 실린다. 마스크는 스스로 그려지지 않아
// 렌더 영역이 없으므로 제 상자로 export 한다.
const clippedByMask = await run('1:8')
assert.deepEqual(clippedByMask.compose.node.mask.area, box(8000, 0, 100, 100))
assert.equal(exports.find((e) => e.id === 'mask').useAbsoluteBounds, true)
// 마스크의 모양을 이루는 레이어는 Figma 가 그리지 않는다. 그 노드의 그림은 싣지 않고
// 조상들만 그린다 — get_screenshot 도 그 아래에 있는 것만 보여 준다.
const shapeOnly = await run('1:11')
assert.ok(shapeOnly.compose, JSON.stringify(shapeOnly))
assert.equal(shapeOnly.compose.node.picture, undefined)
await assert.rejects(referencePng({ nodeId: '9:9' }), /DEVUP_NODE_NOT_FOUND/)
// 답 하나에 들어가지 않는 답은 붙들어 두고 조각으로 내준다. 붙든 글은 ASCII 라
// 오프셋이 여기(UTF-16)와 devup-mcp(바이트)에서 같은 자리를 가리킨다. 셋을 붙들어도
// 먼저 붙든 답이 읽히기 전에 밀려나지 않는다.
const big = new Uint8Array(5 * 1024 * 1024)
big.set(png)
inInset.exportAsync = async () => big
const readBack = async (descriptor) => {
  let text = ''
  for (let piece; !piece || !piece.complete; ) {
    piece = await referencePngChunk({ largeValue: { id: descriptor.id, offset: text.length } })
    assert.equal(piece.offset, text.length)
    text += piece.text
  }
  assert.equal(text.length, descriptor.length)
  return text
}
const heldAnswers = [await run('1:6'), await run('1:6'), await run('1:6')]
assert.deepEqual(heldAnswers.map((answer) => answer.kind), Array(3).fill('devupReferencePngChunked'))
assert.equal(new Set(heldAnswers.map((answer) => answer.id)).size, 3)
const text = await readBack(heldAnswers[0])
assert.match(text, /^[\x20-\x7e]*$/)
assert.equal(JSON.parse(text).byteLength, big.length)
assert.match(JSON.parse(text).note, /TEXT "덮는 글"/)
for (const answer of heldAnswers.slice(1)) await readBack(answer)
await assert.rejects(referencePngChunk({ largeValue: { id: heldAnswers[0].id, offset: 0 } }), /DEVUP_REFERENCE_PNG_ANSWER_GONE/)
// 붙든 답이 상한만큼 쌓여 있으면 새 그림을 그리지 않고 바쁘다고 답한다 — devup-mcp 가
// 다시 묻는다. 2분이 지나도록 아무도 읽지 않은 답은 버린다.
const answers = globalThis.__devupReferencePngAnswers
answers.set('unread', { text: { length: 192 * 1024 * 1024 }, at: Date.now() })
await assert.rejects(run('1:2'), /DEVUP_REFERENCE_PNG_BUSY/)
answers.get('unread').at = Date.now() - 3 * 60 * 1000
assert.equal((await run('1:2')).kind, 'devupReferencePng')
assert.equal(answers.has('unread'), false)
// 조합에 드는 것(조상의 사진 64 MiB)까지 실으면 답 하나의 상한을 넘을 때는 조합을 버리고
// 캔버스 그림만 이유와 함께 보낸다.
const photoBytes = new Uint8Array(64 * 1024 * 1024)
globalThis.figma.getImageByHash = (hash) => (hash === 'big' ? { getBytesAsync: async () => photoBytes } : null)
const canvasBytes = new Uint8Array(4 * 1024 * 1024)
canvasBytes.set(png)
onPhoto.exportAsync = async (settings) => (settings.contentsOnly ? png : canvasBytes)
const overLimit = await run('1:10')
assert.equal(overLimit.kind, 'devupReferencePng')
assert.equal(overLimit.compose, undefined)
assert.equal(overLimit.byteLength, canvasBytes.length)
assert.match(overLimit.approximate, /the answer would be \d+ characters/)
assert.match(overLimit.note, /may show here: TEXT "Caption2"/)
// devup-mcp 가 디코딩하지 않을 사진(여기서는 WebP)은 UI 가 PNG 로 바꿔 보내고 원본
// 크기를 싣는다. PNG 사진은 그대로 보낸다 — 픽셀이 바뀌지 않는다.
const webp = new Uint8Array([0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x57, 0x45, 0x42, 0x50])
const asked = []
globalThis.__devupNormalizeImage = async (bytes, maxEdge) => {
  asked.push([bytes.length, maxEdge])
  return png
}
const fill = (bytes, width, height) => (hash) =>
  hash === 'big' ? { getBytesAsync: async () => bytes, getSizeAsync: async () => ({ width, height }) } : null
onPhoto.exportAsync = async () => png
globalThis.figma.getImageByHash = fill(webp, 3000, 2000)
const decodedFill = await run('1:10')
assert.deepEqual(asked, [[webp.length, 8192]])
assert.deepEqual(decodedFill.compose.images.big, { data: Buffer.from(png).toString('base64'), width: 3000, height: 2000 })
asked.length = 0
globalThis.figma.getImageByHash = fill(png, 2, 2)
const plainFill = await run('1:10')
assert.deepEqual(asked, [])
assert.equal(plainFill.compose.images.big, Buffer.from(png).toString('base64'))
// 바꾸는 데 512 MiB 넘게 들 사진은 UI 에 보내지 않고 그대로 보낸다.
globalThis.figma.getImageByHash = fill(webp, 30000, 30000)
const hugeFill = await run('1:10')
assert.deepEqual(asked, [])
assert.equal(hugeFill.compose.images.big, Buffer.from(webp).toString('base64'))
// 장면 하나의 사진들은 변환 시간 한 몫을 나눠 쓴다. 첫 변환이 그 시간을 다 쓰고 멈추면
// 나머지는 UI 에 보내지 않고 그대로 보낸다 — 캔버스 그림을 보낼 시간이 남는다.
const realNow = Date.now
let clock = realNow()
Date.now = () => clock
const waited = []
globalThis.__devupNormalizeImage = async (bytes, maxEdge, timeoutMs) => {
  waited.push(timeoutMs)
  clock += timeoutMs
  return null
}
const imageFill = (imageHash) => ({ type: 'IMAGE', visible: true, opacity: 1, blendMode: 'NORMAL', imageHash, scaleMode: 'FILL' })
const fills = photo.fills
photo.fills = ['w1', 'w2', 'w3'].map(imageFill)
globalThis.figma.getImageByHash = () => ({ getBytesAsync: async () => webp, getSizeAsync: async () => ({ width: 3000, height: 2000 }) })
const stalled = await run('1:10')
Date.now = realNow
photo.fills = fills
assert.deepEqual(waited, [20000])
assert.deepEqual(Object.values(stalled.compose.images), Array(3).fill(Buffer.from(webp).toString('base64')))
delete globalThis.__devupNormalizeImage
globalThis.figma.getImageByHash = () => null

const unreachable = 'Unable to establish connection to Figma after 10 seconds.'
const refuse = async () => {
  throw new Error(unreachable)
}
globalThis.figma = {
  variables: {
    getLocalVariablesAsync: async () => [],
    getVariableByIdAsync: refuse,
    getVariableCollectionByIdAsync: async () => null,
  },
  getStyleByIdAsync: refuse,
}
const used = await usedResources({
  resources: { variableIds: ['VariableID:1:2'], styles: [{ id: 'S:1,', styleType: 'PAINT' }] },
})
assert.deepEqual(
  used.unresolved.map((entry) => entry.error),
  [unreachable, unreachable],
)

console.log(
  `verified ${EXPECTED.length} generated scripts (pageCatalog, referencePng, referencePngChunk and usedResources executed)`,
)
