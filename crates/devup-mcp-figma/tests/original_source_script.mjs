import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {test} from 'node:test';
execFileSync(process.execPath,[fileURLToPath(new URL('../../../plugin/scripts/gen-scripts.mjs',import.meta.url))],{stdio:'ignore'});
const {SCRIPTS}=await import('../../../plugin/src/generated/scripts.js');
const source=readFileSync(new URL('../src/scripts/assets.js',import.meta.url),'utf8')
 .replace('"__DEVUP_ASSET_RENDER_HELPERS__"',readFileSync(new URL('../src/scripts/asset_render_helpers.js',import.meta.url),'utf8'))
 .replace('"__DEVUP_LARGE_VALUE_HELPERS__"',readFileSync(new URL('../src/scripts/large_value_helpers.js',import.meta.url),'utf8'));
const AsyncFunction=Object.getPrototypeOf(async function(){}).constructor;
const bytes=Uint8Array.from([137,80,78,71,13,10,26,10,0,1,2]);
for(const kind of ['source','compiled']) {
 async function run(overrides={},context=true) {
  let leafExports=0,parentExports=0,reads=0,settings;
  const parent={id:'card',type:'INSTANCE',name:'Card',width:240,height:150,clipsContent:true,cornerRadius:16,
   effects:[{type:'LAYER_BLUR',radius:3}],exportAsync:async s=>{parentExports++;settings=s;return bytes;}};
  const leaf={id:'picture',type:'RECTANGLE',name:'Image',width:120,height:80,parent:context?parent:null,
   fills:[{type:'IMAGE',imageHash:'source',scaleMode:'CROP',imageTransform:[[0.5,0,0.25],[0,0.5,0.1]]}],
   effects:[{type:'LAYER_BLUR',radius:12}],exportAsync:async s=>{leafExports++;settings=s;return bytes;}};
  parent.children=[{id:'mask',isMask:true},leaf];
  const options={assetId:'picture:fills:0',nodeId:'picture',field:'fills/0',imageHash:'source',format:'png',scale:2,transport:'bridge',...overrides};
  const figma={fileKey:'synthetic',getNodeByIdAsync:async id=>id==='card'?parent:leaf,base64Encode:b=>Buffer.from(b).toString('base64'),
   getImageByHash:()=>({getBytesAsync:async()=>{reads++;return bytes;},getSizeAsync:async()=>({width:1000,height:700})})};
  const data=kind==='source'?await new AsyncFunction('figma',source.replace('"__DEVUP_ASSET__"',JSON.stringify(options)))(figma)
   :await (async()=>{globalThis.figma=figma;return SCRIPTS.assets({asset:options});})();
  return {data,leafExports,parentExports,reads,settings};
 }
 test(`${kind}: default source acquires full bytes and ignores render scale without baking`,async()=>{
  const r=await run({scale:1.5});assert.equal(r.data.status,'exported');assert.equal(r.data.renderMode,'original-source');
  assert.equal(r.reads,1);assert.equal(r.leafExports,0);assert.equal(r.parentExports,0);assert.equal(r.data.data,Buffer.from(bytes).toString('base64'));
  assert.equal(r.data.exportDiagnostics.renderInfo.representation,'original-fill');assert.equal(r.data.exportDiagnostics.renderInfo.sourceSize.width,1000);
 });
 test(`${kind}: native fractional and2x retain exact SCALE settings`,async()=>{
  for(const scale of [1,1.5,2,3]){const r=await run({renderMode:'node',scale});assert.equal(r.leafExports,1);assert.equal(r.reads,0);assert.equal(r.settings.constraint.value,scale);assert.equal(r.data.scale,scale);}
 });
 test(`${kind}: visible exports selected ancestor clips/masks/rounding and names scope`,async()=>{
  const r=await run({renderMode:'visible',visibleScopeNodeId:'card',scale:1.5,maxWidth:null,srcset:false});
  assert.equal(r.data.status,'exported');assert.equal(r.leafExports,0);assert.equal(r.parentExports,1);assert.equal(r.settings.constraint.value,1.5);
  const i=r.data.exportDiagnostics.renderInfo;assert.equal(i.captureNodeId,'card');assert.equal(i.captureBox.width,240);assert.equal(i.nodeSize.width,120);
  assert.equal(i.contextScope,'selected-ancestor-subtree');assert.equal(i.visibleContext.canvasReferenceCompared,false);assert.equal(i.effects[0].radius,3);
  assert.equal(r.data.visibleScopeNodeId,'card');
 });
 test(`${kind}: missing/unrelated visible context fails explicitly`,async()=>{
  for(const options of [{renderMode:'visible'},{renderMode:'visible',visibleScopeNodeId:'outside'}]){const r=await run(options,false);assert.equal(r.data.status,'failed');assert.equal(r.leafExports,0);assert.equal(r.parentExports,0);}
 });
 test(`${kind}: direct parent node does not require asset candidacy`,async()=>{
  const r=await run({assetId:'card:node',nodeId:'card',field:'node',imageHash:null,renderMode:'node'});assert.equal(r.data.status,'exported');assert.equal(r.parentExports,1);
 });
 test(`${kind}: arbitrary node cannot masquerade as an original source`,async()=>{
  const r=await run({assetId:'card:node',nodeId:'card',field:'node',imageHash:null});assert.equal(r.data.errorCode,'DEVUP_ORIGINAL_SOURCE_UNAVAILABLE');assert.equal(r.parentExports,0);
 });
}
