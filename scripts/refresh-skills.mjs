// Re-vendors the embedded SKILL.md documents from their source repositories.
//
// The documents under `crates/devup-mcp/src/server/skills/<name>/` are copies.
// The repository each one names is the source of truth, and a copy goes stale
// the moment that repository moves - which is the honest cost of shipping them
// in the binary, and the reason this script exists rather than a note asking
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
// `own` skills are authored in this repository, so the direction reverses:
// nothing is fetched, the file on disk is the truth, and what this script
// updates is the manifest's digest of it. Editing one of those documents and
// running this is the supported cycle; editing one and not running it fails
// the integrity test in `skills.rs`, on purpose.
//
// `external` skills are listed and never fetched. vercel-labs/agent-skills
// ships no LICENSE, so its content is not devup-mcp's to redistribute; the
// manifest carries its install command instead.

import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
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
  if (entry.origin === 'own') {
    // Authored here, so the direction reverses: the file on disk is the truth
    // and the manifest is what has to catch up. There is nothing to fetch, but
    // the digests still have to be resealed after an edit - without this the
    // integrity test in skills.rs fails on your own change and no tool offers
    // to fix it, which is the trap that makes people edit the digest by hand.
    let resealed = 0
    for (const document of entry.documents ?? []) {
      const text = readFileSync(join(skillDir, entry.name, document.path), 'utf8')
      const digest = sha256(text)
      const bytes = Buffer.byteLength(text, 'utf8')
      if (digest === document.sha256 && bytes === document.bytes) continue
      resealed += 1
      drifted += 1
      console.log(
        `~ ${entry.name}/${document.path}: ${document.bytes} -> ${bytes} bytes`,
      )
      if (check) continue
      document.sha256 = digest
      document.bytes = bytes
    }
    if (resealed === 0) {
      console.log(`. ${entry.name}: authored here (${entry.path}), digests already match`)
    }
    continue
  }
  if (entry.origin !== 'embedded') {
    console.log(`- ${entry.name}: external, install with \`${entry.installCommand}\``)
    continue
  }

  // An embedded skill copies one upstream file into its entry document. A
  // multi-document embedded skill would need a per-document upstream path, and
  // refreshing only the first one while reporting the whole skill fresh is the
  // failure worth stopping for rather than working around.
  if (entry.documents?.length !== 1 || entry.documents[0].path !== 'SKILL.md') {
    throw new Error(
      `${entry.name}: an embedded skill must have exactly one SKILL.md document; ` +
        `give each document its own upstream path before vendoring more of them.`,
    )
  }
  const document = entry.documents[0]

  const text = await github(`repos/${entry.repo}/contents/${entry.path}`, true)
  const [head] = await github(
    `repos/${entry.repo}/commits?path=${encodeURIComponent(entry.path)}&per_page=1`,
  )
  const digest = sha256(text)

  if (digest === document.sha256) {
    console.log(`= ${entry.name}: unchanged at ${entry.commit.slice(0, 12)}`)
    continue
  }

  drifted += 1
  console.log(
    `~ ${entry.name}: ${entry.commit.slice(0, 12)} -> ${head.sha.slice(0, 12)} ` +
      `(${document.bytes} -> ${Buffer.byteLength(text, 'utf8')} bytes)`,
  )
  if (check) continue

  const target = join(skillDir, entry.name, document.path)
  mkdirSync(dirname(target), { recursive: true })
  writeFileSync(target, text, 'utf8')
  entry.commit = head.sha
  entry.committedAt = head.commit.committer.date
  document.sha256 = digest
  document.bytes = Buffer.byteLength(text, 'utf8')
  entry.sourceUrl = `https://github.com/${entry.repo}/blob/${head.sha}/${entry.path}`
}

if (!check && drifted > 0) {
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8')
  console.log(
    `\nReconciled ${drifted} document(s) and rewrote the manifest. Commit both: a vendored ` +
      `copy moved forward, an authored one had its digest resealed, or both.`,
  )
} else if (check && drifted > 0) {
  console.error(
    `\n${drifted} document(s) disagree with their source of truth - upstream for a vendored ` +
      `skill, the file on disk for an authored one. Run without --check to reconcile.`,
  )
  process.exit(1)
} else {
  console.log('\nEvery document agrees with its source of truth.')
}
