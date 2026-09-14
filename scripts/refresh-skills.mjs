// Re-vendors the embedded SKILL.md documents from their source repositories.
//
// The documents under `crates/devup-mcp/src/server/skills/` are copies. The
// repository each one names is the source of truth, and a copy goes stale the
// moment that repository moves - which is the honest cost of shipping them in
// the binary, and the reason this script exists rather than a note asking
// someone to remember.
//
// Run it, commit what changed, and the integrity test in `skills.rs` will
// confirm the manifest and the documents agree. Editing a vendored document by
// hand instead will fail that test, which is the point: the fix belongs
// upstream, not in the copy.
//
//   node scripts/refresh-skills.mjs           # rewrite the embedded documents
//   node scripts/refresh-skills.mjs --check   # report drift, write nothing
//
// External skills are listed and never fetched. vercel-labs/agent-skills ships
// no LICENSE, so its content is not devup-mcp's to redistribute; the manifest
// carries its install command instead.

import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const skillDir = resolve(here, '../crates/devup-mcp/src/server/skills')
const manifestPath = join(skillDir, 'manifest.json')
const check = process.argv.includes('--check')

const sha256 = (text) => createHash('sha256').update(text, 'utf8').digest('hex')

async function github(path, raw = false) {
  const response = await fetch(`https://api.github.com/${path}`, {
    headers: {
      accept: raw ? 'application/vnd.github.raw' : 'application/vnd.github+json',
      'user-agent': 'devup-mcp-refresh-skills',
      ...(process.env.GITHUB_TOKEN
        ? { authorization: `Bearer ${process.env.GITHUB_TOKEN}` }
        : {}),
    },
  })
  if (!response.ok) {
    throw new Error(`GET ${path} -> ${response.status} ${response.statusText}`)
  }
  return raw ? response.text() : response.json()
}

const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
let drifted = 0

for (const entry of manifest.skills) {
  if (entry.origin !== 'embedded') {
    console.log(`- ${entry.name}: external, install with \`${entry.installCommand}\``)
    continue
  }

  const text = await github(`repos/${entry.repo}/contents/${entry.path}`, true)
  const [head] = await github(
    `repos/${entry.repo}/commits?path=${encodeURIComponent(entry.path)}&per_page=1`,
  )
  const digest = sha256(text)

  if (digest === entry.sha256) {
    console.log(`= ${entry.name}: unchanged at ${entry.commit.slice(0, 12)}`)
    continue
  }

  drifted += 1
  console.log(
    `~ ${entry.name}: ${entry.commit.slice(0, 12)} -> ${head.sha.slice(0, 12)} ` +
      `(${entry.bytes} -> ${Buffer.byteLength(text, 'utf8')} bytes)`,
  )
  if (check) continue

  writeFileSync(join(skillDir, `${entry.name}.md`), text, 'utf8')
  entry.commit = head.sha
  entry.committedAt = head.commit.committer.date
  entry.sha256 = digest
  entry.bytes = Buffer.byteLength(text, 'utf8')
  entry.sourceUrl = `https://github.com/${entry.repo}/blob/${head.sha}/${entry.path}`
}

if (!check && drifted > 0) {
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8')
  console.log(`\nRewrote ${drifted} document(s) and the manifest. Commit both.`)
} else if (check && drifted > 0) {
  console.error(`\n${drifted} vendored document(s) are behind their source.`)
  process.exit(1)
} else {
  console.log('\nEvery vendored document matches its source.')
}
