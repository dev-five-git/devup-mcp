// Figma 의 UI iframe 은 `srcdoc` 으로 주입되므로 상대 경로 스크립트를 가져올 수
// 없다. 번들이 `<script src>` 로 남아 있으면 창은 뜨지만 아무 동작도 하지 않고,
// 원인도 드러나지 않는다. 그래서 빌드 뒤 실제로 본문에 박아 넣는다.

import { readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const dist = resolve(dirname(fileURLToPath(import.meta.url)), '../dist')
const htmlPath = join(dist, 'ui.html')
const jsPath = join(dist, 'ui.js')

const html = readFileSync(htmlPath, 'utf8')
const js = readFileSync(jsPath, 'utf8')

const tag = /<script[^>]*\ssrc="[^"]*ui\.js"[^>]*><\/script>/
if (!tag.test(html)) {
  throw new Error('ui.html 에서 ui.js 참조를 찾지 못했습니다 — 빌드 설정 확인')
}

// `</script>` 가 번들 문자열 안에 있으면 HTML 파서가 거기서 블록을 닫는다.
const safe = js.split('</script>').join('<\\/script>')

writeFileSync(htmlPath, html.replace(tag, `<script>${safe}</script>`), 'utf8')
rmSync(jsPath, { force: true })
rmSync(`${jsPath}.map`, { force: true })

console.log(`inlined ${js.length} bytes into ui.html`)
