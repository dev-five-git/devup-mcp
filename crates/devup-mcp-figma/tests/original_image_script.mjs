import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { createHash } from 'node:crypto';

const source = readFileSync(new URL('../src/scripts/assets.js', import.meta.url), 'utf8')
  .replace('"__DEVUP_LARGE_VALUE_HELPERS__"', readFileSync(new URL('../src/scripts/large_value_helpers.js', import.meta.url), 'utf8'));
const bytes = Uint8Array.from([255, 216, 255, 224, 1, 2, 3]);
async function read(overrides = {}, api = {}) {
  const options = { assetId: 'n:original:2', nodeId: 'n', field: '$original-image/fills/2', imageHash: 'active', format: 'png', scale: 1, version: 'v1', transport: 'bridge', ...overrides };
  const figma = {
    fileKey: 'file', base64Encode: b => Buffer.from(b).toString('base64'),
    getNodeByIdAsync: async () => ({ fills: [{type:'IMAGE', imageHash:'hidden', visible:false}, {type:'SOLID'}, {type:'IMAGE', imageHash:'active'}], exportAsync: async () => { throw Error('must not render the node'); } }),
    getImageByHash: hash => { assert.equal(hash, 'active'); return { getBytesAsync: async () => bytes, getSizeAsync: async () => ({width:13, height:17}) }; },
    ...api,
  };
  return new (Object.getPrototypeOf(async function(){}).constructor)('figma', source.replace('"__DEVUP_ASSET__"', JSON.stringify(options)))(figma);
}
test('bridge returns selected original bytes and intrinsic codec, never a node PNG', async () => {
  const result = await read();
  assert.equal(result.status, 'exported');
  assert.equal(result.representation, 'original-image-v1');
  assert.equal(result.mimeType, 'image/jpeg');
  assert.equal(result.width, 13);
  assert.equal(result.height, 17);
  assert.equal(result.data, Buffer.from(bytes).toString('base64'));
  assert.equal(result.sha256, createHash('sha256').update(bytes).digest('hex'));
});
test('remote refuses original bytes before reading them', async () => {
  const result = await read({transport:undefined}, {getImageByHash: () => { throw Error('remote must refuse first'); }});
  assert.equal(result.errorCode, 'DEVUP_ORIGINAL_IMAGE_REQUIRES_BRIDGE');
});
test('stale selected paint is refused', async () => {
  assert.equal((await read({imageHash:'stale'})).errorCode, 'DEVUP_ASSET_SOURCE_CHANGED');
});
test('missing source image is diagnosed distinctly', async () => {
  assert.equal((await read({}, {getImageByHash: () => null})).errorCode, 'DEVUP_ORIGINAL_IMAGE_NOT_FOUND');
});
test('source reads obey the byte cap and do not reinterpret unknown codecs', async () => {
  for (const [data, expected] of [[new Uint8Array(), 'DEVUP_ASSET_RESPONSE_TOO_LARGE'],
      [new Uint8Array(8 * 1024 * 1024 + 1), 'DEVUP_ASSET_RESPONSE_TOO_LARGE'],
      [new Uint8Array([1,2,3,4]), 'DEVUP_ORIGINAL_IMAGE_CODEC_UNSUPPORTED']]) {
    const result = await read({}, {getImageByHash: () => ({getBytesAsync:async()=>data})});
    assert.equal(result.errorCode, expected);
  }
});
test('source API rejection is named as a read failure, not a node export failure', async () => {
  const result = await read({}, {getImageByHash: () => ({getBytesAsync:async()=>{throw Error('read failed');}})});
  assert.equal(result.errorCode, 'DEVUP_ORIGINAL_IMAGE_READ_FAILED');
});
