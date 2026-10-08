// 생성된 스크립트를 실제로 불러와 실행해 본다.
//
// 타입체크와 번들만으로는 코드젠이 옳았는지 알 수 없다. 최상위 `return` 이 함수
// 밖에 놓이거나 플레이스홀더가 이상한 값으로 바뀌면 번들은 그대로 나오고 Figma
// 안에서야 터진다. 그때는 원인이 플러그인인지 스크립트인지 가리기 어렵다.
//
// 그래서 여기서 한 번 불러오고(구조가 깨졌으면 import 가 실패한다), 최소한의
// figma 스텁으로 실행까지 해 본다.

import assert from 'node:assert/strict'

const { SCRIPTS, pageCatalog, referencePng } = await import('../src/generated/scripts.js')

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

// 참조 PNG 는 수집기가 공식 `get_screenshot` 과 같은 그림으로 받으므로, 어떤
// 설정으로 export 하는지가 계약이다.
let exported
const drawn = (width, height) => ({
  absoluteBoundingBox: { x: 0, y: 0, width, height },
  exportAsync: async (settings) => {
    exported = settings
    return new Uint8Array([137, 80, 78, 71])
  },
})
const nodes = { '1:2': drawn(360, 740), '1:3': drawn(1920, 9000) }
globalThis.figma = {
  getNodeByIdAsync: async (id) => nodes[id] ?? null,
  base64Encode: (bytes) => Buffer.from(bytes).toString('base64'),
}
const reference = await referencePng({ nodeId: '1:2' })
assert.deepEqual(exported, {
  format: 'PNG',
  constraint: { type: 'SCALE', value: 1 },
  useAbsoluteBounds: true,
  colorProfile: 'SRGB',
})
assert.deepEqual(reference, {
  kind: 'devupReferencePng',
  nodeId: '1:2',
  mimeType: 'image/png',
  byteLength: 4,
  data: 'iVBORw==',
})
await referencePng({ nodeId: '1:3' })
assert.deepEqual(exported.constraint, { type: 'HEIGHT', value: 8192 })
await assert.rejects(referencePng({ nodeId: '9:9' }), /DEVUP_NODE_NOT_FOUND/)

console.log(`verified ${EXPECTED.length} generated scripts (pageCatalog and referencePng executed)`)
