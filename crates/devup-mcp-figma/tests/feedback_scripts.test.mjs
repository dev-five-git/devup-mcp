import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const src = new URL('../src/', import.meta.url);
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
function script(name, parameters) {
  let source = readFileSync(new URL(`scripts/${name}.js`, src), 'utf8');
  for (const [key, file] of Object.entries({
    __DEVUP_ASSET_RENDER_HELPERS__: 'scripts/asset_render_helpers.js',
    __DEVUP_COMPONENT_IDENTITY_HELPERS__: 'scripts/component_identity_helpers.js',
    __DEVUP_LARGE_VALUE_HELPERS__: 'scripts/large_value_helpers.js',
    __DEVUP_PLUGIN_API_MANIFEST__: 'plugin_api_manifest.json',
    __DEVUP_TEXT_SEGMENT_MANIFEST__: 'text_segment_manifest.json',
  })) source = source.replaceAll(`"${key}"`, () => readFileSync(new URL(file, src), 'utf8'));
  for (const [key, value] of Object.entries(parameters)) source = source.replaceAll(`"${key}"`, () => JSON.stringify(value));
  return new AsyncFunction('figma', source);
}

const id = 'I44:8533;10:973';
const options = { assetId: `${id}:node`, nodeId: id, field: 'node', format: 'svg', scale: 1, timeoutMs: 1000, transport: 'bridge' };
function vector() {
  return { id, type: 'VECTOR', width: 32, height: 24, opacity: 1,
    absoluteTransform: [[1, 0, 0], [0, 1, 0]], relativeTransform: [[1, 0, 0], [0, 1, 0]],
    absoluteRenderBounds: { x: 0, y: 0, width: 32, height: 24 },
    fills: [{ type: 'SOLID', color: { r: 1, g: 0, b: 0 } }], strokes: [], effects: [],
    fillGeometry: [{ windingRule: 'NONZERO', data: 'M0 0H32V24H0Z' }],
    exportAsync: async () => { throw Error('actual nested renderer exception'); } };
}
async function asset(node, lookup = async () => node) {
  return script('assets', { __DEVUP_ASSET__: options })({ fileKey: 'fixture', getNodeByIdAsync: lookup, base64Encode: b => Buffer.from(b).toString('base64') });
}
test('nested asset SVG recovery preserves actual subtree geometry and exception evidence', async () => {
  const node = vector();
  const result = await asset(node, async requested => requested === id ? null : { id: '44:8533', children: [node] });
  assert.equal(result.status, 'exported');
  assert.equal(result.exportDiagnostics.method, 'geometry');
  assert.match(result.exportDiagnostics.nativeError, /actual nested renderer exception/);
  assert.match(result.text, /M0 0H32V24H0Z/);
});
test('unsupported fallback paint remains a failure with the real native cause', async () => {
  const node = vector();
  node.fills = [{ type: 'GRADIENT_LINEAR' }];
  const result = await asset(node);
  assert.equal(result.status, 'failed');
  assert.match(result.errorDetail, /actual nested renderer exception/);
  assert.equal(result.data, undefined);
});
test('native SVG remains authoritative', async () => {
  const node = vector();
  const original = '<svg><path d="M9 8L7 6"/></svg>';
  node.exportAsync = async () => original;
  const result = await asset(node);
  assert.equal(result.status, 'exported');
  assert.equal(result.text, original);
  assert.equal(result.exportDiagnostics.method, 'native');
});
for (const name of ['snapshot', 'fast_snapshot']) {
  test(`${name} acquires async component identities and preserves actual lookup failures`, async () => {
    const root = { id: '1:1', type: 'FRAME', name: 'screen', children: [] };
    const main = { id: '10:1', name: 'Header', parent: { id: '10:0', type: 'COMPONENT_SET' } };
    root.children = [
      { id: '1:2', type: 'INSTANCE', parent: root, children: [], getMainComponentAsync: async () => main },
      { id: '1:3', type: 'INSTANCE', parent: root, children: [], getMainComponentAsync: async () => { throw Error('actual identity lookup exception'); } },
    ];
    const result = await script(name, { __DEVUP_NODE_ID__: '1:1', __DEVUP_ROOT_IDS__: ['1:1'], __DEVUP_SNAPSHOT__: {} })({ fileKey: 'fixture', getNodeByIdAsync: async () => root, variables: {} });
    const nodes = (result.snapshot ?? result).nodes;
    assert.equal(nodes.find(n => n.id === '1:2').fields.mainComponentId, '10:1');
    assert.equal(nodes.find(n => n.id === '1:2').fields.mainComponentSetId, '10:0');
    assert.equal(nodes.find(n => n.id === '1:3').fieldErrors.mainComponentId, 'actual identity lookup exception');
  });
}

test('direct Desktop capture includes a numeric range sibling and excludes unrelated frames', async () => {
  const section = {id:'0:1',type:'SECTION',children:[]};
  const make = (id,name,width) => ({id,name,width,height:900,type:'FRAME',parent:section,children:[]});
  section.children = [make('1:1','Desktop',1440),make('1:2','992~1280',992),make('1:3','Mobile',360),make('1:4','Frame 14',992),make('1:5','1280~992',992)];
  const result = await script('fast_snapshot', {__DEVUP_ROOT_IDS__:['1:1'],__DEVUP_SNAPSHOT__:{}})({fileKey:'fixture',getNodeByIdAsync:async id=>section.children.find(n=>n.id===id),variables:{}});
  assert.deepEqual(new Set(result.snapshot.rootIds),new Set(['1:1','1:2','1:3']));
});
