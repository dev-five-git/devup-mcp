// devup-mcp 의 Figma 읽기 스크립트를 플러그인이 부를 수 있는 함수로 바꾼다.
//
// 스크립트 원본은 `crates/devup-mcp-figma/src/scripts/*.js` 하나뿐이다. 원격
// 경로(공식 Figma MCP `use_figma`)는 Rust 가 플레이스홀더를 값으로 치환해 코드
// 문자열을 보내고, 브리지 경로는 이 파일이 같은 원본을 빌드 타임에 함수로 감싼다.
// 원본이 둘이 되면 두 경로의 봉투가 조용히 갈라지므로 복제하지 않는다.
//
// 스크립트는 최상위 `await` 와 최상위 `return` 을 쓰는 async 함수 "본문"이다.
// 그래서 감싸기만 하면 그대로 동작한다. eval 은 쓰지 않는다 — Figma 플러그인
// 샌드박스에서 동적 코드 실행은 막혀 있고, 번들러가 정적으로 볼 수 있어야 한다.

import { mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const pluginRoot = resolve(here, '..')
const figmaCrate = resolve(pluginRoot, '../crates/devup-mcp-figma/src')
const scriptDir = join(figmaCrate, 'scripts')
// 확장자는 붙이지 않는다 — `.js` 와 `.d.ts` 두 벌로 나간다.
const outFile = join(pluginRoot, 'src/generated/scripts')

/**
 * 런타임 파라미터 플레이스홀더 → 함수 인자 경로.
 *
 * Rust 의 `BuiltinScript::source` 가 채우는 값과 같은 자리다. 이름이 갈라지면
 * 브리지 경로만 조용히 빈 값으로 동작하므로 Rust 쪽과 함께 고쳐야 한다.
 */
const PARAMS = {
  __DEVUP_NODE_ID__: 'p.nodeId',
  __DEVUP_ROOT_IDS__: 'p.rootIds',
  __DEVUP_SNAPSHOT__: 'p.snapshot',
  __DEVUP_SEARCH__: 'p.search',
  __DEVUP_EXPLORE__: 'p.explore',
  __DEVUP_RESOURCE_BATCH__: 'p.resources',
  __DEVUP_LARGE_VALUE__: 'p.largeValue',
  __DEVUP_ASSET__: 'p.asset',
  __DEVUP_THEME__: 'p.theme',
}

/**
 * 빌드 타임에 그대로 박히는 인클루드. 값이 아니라 소스/스키마라 파라미터가 아니다.
 *
 * `__DEVUP_SNAPSHOT_CURSOR__` 는 여기에도 위에도 없다. 그것은 치환 대상이 아니라
 * 스크립트와 Rust(`snapshot.rs` 의 `SNAPSHOT_CURSOR_ID`)가 공유하는 마커 노드
 * id 리터럴이다. 치환하면 페이지네이션이 조용히 깨진다.
 */
const INCLUDES = {
  __DEVUP_LARGE_VALUE_HELPERS__: () =>
    readFileSync(join(scriptDir, 'large_value_helpers.js'), 'utf8'),
  __DEVUP_PLUGIN_API_MANIFEST__: () =>
    readFileSync(join(figmaCrate, 'plugin_api_manifest.json'), 'utf8'),
  __DEVUP_TEXT_SEGMENT_MANIFEST__: () =>
    readFileSync(join(figmaCrate, 'text_segment_manifest.json'), 'utf8'),
}

/** `fast_snapshot.js` → `fastSnapshot`. Rust 의 `BuiltinScript` 이름과 맞춘다. */
const camel = (file) =>
  file.replace(/\.js$/, '').replace(/_([a-z])/g, (_, c) => c.toUpperCase())

/** 본문에 남은 미치환 플레이스홀더가 있으면 빌드를 세운다. */
function assertNoLeftovers(file, body) {
  const left = [...body.matchAll(/"__DEVUP_[A-Z_]+__"/g)]
    .map((m) => m[0])
    // 마커 노드 id 는 리터럴로 남는 것이 정상이다.
    .filter((token) => token !== '"__DEVUP_SNAPSHOT_CURSOR__"')
  if (left.length > 0) {
    throw new Error(
      `${file}: 치환되지 않은 플레이스홀더 ${[...new Set(left)].join(', ')} — ` +
        'gen-scripts.mjs 의 PARAMS/INCLUDES 에 추가해야 한다',
    )
  }
}

function transform(file) {
  let body = readFileSync(join(scriptDir, file), 'utf8')
  for (const [token, read] of Object.entries(INCLUDES)) {
    body = body.split(`"${token}"`).join(read())
  }
  for (const [token, expr] of Object.entries(PARAMS)) {
    body = body.split(`"${token}"`).join(expr)
  }
  assertNoLeftovers(file, body)
  return body
}

const files = readdirSync(scriptDir)
  .filter((f) => f.endsWith('.js'))
  // 단독 실행되지 않고 다른 스크립트에 인클루드되기만 한다.
  .filter((f) => f !== 'large_value_helpers.js')
  .sort()

// 본문은 손대지 않은 JavaScript다. `.ts` 로 내보내면 검증된 스크립트가 strict
// 검사에 걸려 수백 건의 암시적 any 를 낸다 — 고칠 대상은 devup-mcp 의 원본이지
// 이 생성물이 아니므로, 실제 언어대로 `.js` 로 내보내고 타입은 경계에만 둔다.
const banner = `// 자동 생성 파일 — 직접 고치지 마십시오.
// 원본: crates/devup-mcp-figma/src/scripts/*.js
// 생성: plugin/scripts/gen-scripts.mjs (\`npm run gen\`)
//
// 원격 경로(공식 Figma MCP)와 브리지 경로가 같은 스크립트를 쓰도록 빌드 타임에
// 감싼 결과입니다. 원본을 고치면 이 파일은 다시 생성됩니다.`

const js = `${banner}

${files
  .map(
    (file) => `/** 원본: ${file} */
export async function ${camel(file)}(p) {
${transform(file)}
}`,
  )
  .join('\n\n')}

export const SCRIPTS = {
${files.map((f) => `  ${camel(f)},`).join('\n')}
}
`

const dts = `${banner}

/** 스크립트가 읽는 값. Rust 의 \`ScriptInputs\` 와 자리를 맞춘다. */
export interface ScriptParams {
  nodeId: string
  rootIds: string[]
  snapshot: unknown
  search: unknown
  explore: unknown
  resources: unknown
  largeValue: unknown
  asset: unknown
  theme: unknown
}

${files.map((f) => `export declare function ${camel(f)}(p: ScriptParams): Promise<unknown>`).join('\n')}

/** 스크립트 이름 → 실행 함수. devup-mcp 가 보내는 \`script\` 값이 키다. */
export declare const SCRIPTS: Record<
  string,
  (p: ScriptParams) => Promise<unknown>
>
`

mkdirSync(dirname(outFile), { recursive: true })
writeFileSync(`${outFile}.js`, js, 'utf8')
writeFileSync(`${outFile}.d.ts`, dts, 'utf8')
console.log(`generated ${files.length} scripts -> ${outFile}.{js,d.ts}`)
