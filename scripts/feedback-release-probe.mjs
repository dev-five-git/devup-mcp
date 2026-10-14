// Read-only Prime acquisition and source validation through an explicitly named
// local binary. Generated evidence/assets stay inside the integration checkout.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';

const [binary, projectRoot] = process.argv.slice(2);
if (!binary || !projectRoot) throw Error('Usage: node scripts/feedback-release-probe.mjs <binary> <Prime project root>');
const out = resolve('target/feedback-release/live');
mkdirSync(out, { recursive: true });
const child = spawn(resolve(binary), ['--allow-write-root', process.cwd()], {
  env: { ...process.env, DEVUP_MCP_NO_AUTO_UPDATE: '1', DEVUP_MCP_SKILLS_OFFLINE: '1', DEVUP_FIGMA_CREDENTIAL_STORE: 'memory', DEVUP_ASSET_TIMEOUT_MS: '180000' },
  stdio: ['pipe', 'pipe', 'pipe'],
});
const pending = new Map();
let nextId = 0;
let stderrBytes = 0;
child.stderr.on('data', b => { stderrBytes += b.length; });
createInterface({ input: child.stdout }).on('line', line => {
  const reply = JSON.parse(line);
  const request = pending.get(reply.id);
  if (request) { clearTimeout(request.timer); pending.delete(reply.id); request.resolve(reply); }
});
child.on('error', error => { for (const p of pending.values()) p.reject(error); });
function rpc(method, params) {
  return new Promise((resolve, reject) => {
    const id = ++nextId;
    const timer = setTimeout(() => { pending.delete(id); reject(Error(`${method} timed out after 60 seconds`)); }, 60000);
    pending.set(id, { resolve, reject, timer });
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  });
}
async function tool(name, arguments_) {
  const raw = await rpc('tools/call', { name, arguments: arguments_ });
  if (raw.error) throw Error(JSON.stringify(raw.error));
  const data = raw.result.structuredContent ?? JSON.parse(raw.result.content.find(c => c.type === 'text').text);
  return { data, wireBytes: Buffer.byteLength(JSON.stringify(raw.result)) };
}
async function exported(arguments_) {
  let reply = await tool('devup_figma_export', arguments_);
  const deadline = Date.now() + 300000;
  while (reply.data.status === 'in_progress') {
    if (Date.now() > deadline) throw Error('Export polling exceeded five minutes');
    await new Promise(resolve => setTimeout(resolve, 150));
    const jobId = reply.data.exportJob?.jobId ?? reply.data.assetJob?.jobId;
    if (!jobId) throw Error('Running export has no job ID');
    reply = await tool('devup_figma_export', { jobId, projectRoot });
  }
  return reply;
}
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const url = node => `https://www.figma.com/design/YVPOYYXZJpOayp0QlCdIVT/x?node-id=${encodeURIComponent(node)}`;
async function resource(link) {
  const manifestReply = await rpc('resources/read', { uri: link.uri });
  const manifest = JSON.parse(manifestReply.result.contents[0].text);
  const chunks = [];
  for (const uri of manifest.chunkUris) {
    const reply = await rpc('resources/read', { uri });
    const content = reply.result.contents[0];
    chunks.push(content.blob ? Buffer.from(content.blob, 'base64') : Buffer.from(content.text));
  }
  const bytes = Buffer.concat(chunks);
  assert.equal(bytes.length, link.size);
  assert.equal(hash(bytes), link.contentHash);
  return bytes;
}
function summary(reply) {
  const r = reply.data;
  return { status: r.status, error: r.error, server: r.server, wireBytes: reply.wireBytes,
    source: r.source, quality: r.quality, collection: r.collection, responsiveGroups: r.responsiveGroups,
    breakpointCompletion: r.breakpointCompletion, interactionCoverage: r.interactionCoverage,
    interactionEvidenceLimits: r.interactionEvidenceLimits, candidates: r.interactiveCandidates,
    sharedComponentCandidates: r.sharedComponentCandidates, widgetCandidates: r.widgetCandidates,
    ambiguityQuestions: r.ambiguityQuestions, reviewChecklist: r.reviewChecklist,
    devupJsonMode: r.devupJsonMode, skippedStyles: r.skippedStyles, unresolvedUsedTokens: r.unresolvedUsedTokens,
    themeDiff: r.themeDiff, projectContext: r.projectContext, completionEvidence: r.completionEvidence };
}
const evidence = { at: new Date().toISOString(), binary: resolve(binary), projectRoot: resolve(projectRoot),
  environment: 'New integration binary; existing local bridge relay; direct credential store disabled; no design mutation or frontend write', assets: {}, exports: {}, validations: {}, comparisons: { ran: false, reason: 'This probe does not launch a browser or establish pixel/runtime equivalence.' } };
try {
  evidence.initialize = await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'feedback-release-live', version: '1' } });
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
  evidence.toolCount = (await rpc('tools/list', {})).result.tools.length;
  evidence.auth = (await tool('devup_figma_auth', { action: 'status' })).data;
  for (const format of ['svg', 'png', 'pdf']) {
    const outputPath = join(out, `prime-logo.${format}`);
    const reply = await exported({ url: url('I44:8533;10:973'), outputs: ['assetManifest'],
      assetRequests: [{ assetId: 'I44:8533;10:973:node', format, scale: format === 'png' ? 2 : 1, outputPath }], delivery: 'inline', refresh: true });
    const record = evidence.assets[format] = { ...summary(reply), manifest: reply.data.assetManifest, job: reply.data.assetJob };
    const asset = reply.data.assetManifest?.assets?.find(a => a.assetId === 'I44:8533;10:973:node');
    if (asset?.status === 'exported' && existsSync(outputPath)) {
      const bytes = readFileSync(outputPath);
      record.file = { byteLength: bytes.length, sha256: hash(bytes), validSignature: format === 'svg' ? /<svg[\s>]/.test(bytes.toString()) && /<path[\s>]/.test(bytes.toString()) : format === 'png' ? bytes.subarray(0, 8).toString('hex') === '89504e470d0a1a0a' : bytes.subarray(0, 5).toString() === '%PDF-' };
      assert.equal(record.file.sha256, asset.sha256);
      assert.equal(record.file.byteLength, asset.byteLength);
      assert.equal(record.file.validSignature, true);
    }
    console.log(JSON.stringify({ asset: format, status: reply.data.status, file: record.file, error: reply.data.error }));
  }
  for (const [name, arguments_] of [
    ['section', { url: url('38:5796'), outputs: ['tsx'], delivery: 'auto', refresh: true }],
    ['desktop-used', { url: url('38:5207'), outputs: ['tsx', 'devupJson', 'sourceMap', 'rawSnapshot'], debug: true, devupJsonMode: 'used-only', delivery: 'auto', refresh: true,
      outputPaths: { tsx: join(out, 'desktop.tsx'), devupJson: join(out, 'desktop-devup.json') } }],
    ['maintenance', { url: url('33:3908'), outputs: ['tsx'], delivery: 'auto', refresh: true }],
  ]) {
    const reply = await exported({ ...arguments_, projectRoot });
    const record = evidence.exports[name] = summary(reply);
    record.resources = [];
    for (const link of reply.data.resources ?? []) {
      const bytes = await resource(link);
      const file = reply.data.outputPaths?.[link.name];
      record.resources.push({ name: link.name, byteLength: bytes.length, sha256: hash(bytes), fileMatches: file ? bytes.equals(readFileSync(file)) : null });
      if (file) assert.equal(record.resources.at(-1).fileMatches, true);
      if (link.name === 'raw-snapshot.json') {
        const snapshot = JSON.parse(bytes);
        record.componentIdentity = Object.values(snapshot.nodes).filter(n => n.nodeType === 'INSTANCE' || n.type === 'INSTANCE').map(n => ({ nodeId: n.id, mainComponentId: n.fields?.mainComponentId, mainComponentName: n.fields?.mainComponentName, mainComponentSetId: n.fields?.mainComponentSetId, fieldError: n.fieldErrors?.mainComponentId }));
      }
      if (link.name === 'tsx') record.codeHints = { interactionCommentCount: (bytes.toString().match(/\/\/ @devup-interaction /g) ?? []).length };
    }
    console.log(JSON.stringify({ export: name, status: record.status, wireBytes: record.wireBytes, candidates: record.interactionCoverage, error: record.error }));
  }
  const arrowPath = join(out, 'normal-arrow.svg');
  const arrowReply = await exported({ url: url('I38:6538;30:1937;30:843'), outputs: ['assetManifest'], assetRequests: [{ assetId: 'I38:6538;30:1937;30:843:node', format: 'svg', scale: 1, outputPath: arrowPath }], delivery: 'auto', refresh: true });
  evidence.assets.arrow = { ...summary(arrowReply), file: existsSync(arrowPath) ? { byteLength: readFileSync(arrowPath).length, sha256: hash(readFileSync(arrowPath)) } : null };
  evidence.routes = (await tool('devup_project_context', { scope: 'routes', projectRoot })).data;
  evidence.ui = (await tool('devup_project_context', { scope: 'ui', projectRoot })).data;
  for (const path of ['src/components/layout/Header.tsx', 'src/components/pages/services/ServicePage.tsx']) {
    const absolute = join(projectRoot, path);
    if (existsSync(absolute)) {
      const source = readFileSync(absolute, 'utf8');
      const reply = await tool('devup_ui_validate', { tsx: source, sourceName: path, projectRoot });
      evidence.validations[path] = { sourceSha256: hash(source), ok: reply.data.ok, violationCounts: reply.data.violationCounts, navigation: reply.data.navigation, server: reply.data.server };
    }
  }
  evidence.missingRouteProbe = (await tool('devup_ui_validate', { tsx: "import Link from 'next/link'; export default ()=> <Link href='/inquiry'>견적문의</Link>", projectRoot })).data;
} catch (error) {
  evidence.blocker = error.message;
  console.error(error.message);
  process.exitCode = 1;
} finally {
  evidence.stderrBytes = stderrBytes;
  writeFileSync(join(out, 'evidence.json'), JSON.stringify(evidence, null, 2) + '\n');
  child.stdin.end();
  child.kill();
}
