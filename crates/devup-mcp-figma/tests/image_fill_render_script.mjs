import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// No npm install is needed: the generator uses only Node built-ins. Enforce
// fresh compiled wrappers even in a clean Windows/Linux cargo test checkout.
execFileSync(process.execPath,[fileURLToPath(new URL('../../../plugin/scripts/gen-scripts.mjs',import.meta.url))],{stdio:'ignore'});
const { SCRIPTS } = await import('../../../plugin/src/generated/scripts.js');

const source = readFileSync(new URL('../src/scripts/assets.js', import.meta.url), 'utf8')
  .replace('"__DEVUP_ASSET_RENDER_HELPERS__"', readFileSync(new URL('../src/scripts/asset_render_helpers.js', import.meta.url), 'utf8'))
  .replace('"__DEVUP_LARGE_VALUE_HELPERS__"', readFileSync(new URL('../src/scripts/large_value_helpers.js', import.meta.url), 'utf8'));
const AsyncFunction = Object.getPrototypeOf(async function(){}).constructor;
const raw = Uint8Array.from([137,80,78,71,13,10,26,10,1,2,3]);
const rendered = Uint8Array.from([137,80,78,71,13,10,26,10,4,5,6]);

for (const implementation of ['source', 'compiled-plugin']) {
  async function run(overrides = {}, changes = {}, api = {}) {
    const options = {assetId:'sample:fills:0', nodeId:'sample', field:'fills/0', imageHash:'source', format:'png', scale:1, transport:'bridge', ...overrides};
    let exports = 0, reads = 0, settings;
    const node = { id:'sample', type:'RECTANGLE', width:100, height:80,
      fills:[{type:'IMAGE', imageHash:'source', scaleMode:'CROP', imageTransform:[[0.5,0,0.25],[0,1,0]], filters:{contrast:0.2}}],
      effects:[{type:'LAYER_BLUR', radius:8, visible:true},{type:'BACKGROUND_BLUR', radius:4, visible:true}],
      isMask:true, clipsContent:true,
      parent:{type:'FRAME', clipsContent:true, effects:[{type:'DROP_SHADOW'}], children:[{isMask:true}]},
      exportAsync:async function(value) {
        assert.equal(this,node,'the exact looked-up node must be exported');
        assert.equal(JSON.stringify([this.fills,this.effects,this.isMask,this.clipsContent]),initialRendering,'paint transforms, blur/effects and masks must remain intact at the native call');
        exports++; settings=value; if (changes.reject) throw Error('renderer refused'); return rendered;
      },
      ...changes };
    const initialRendering=JSON.stringify([node.fills,node.effects,node.isMask,node.clipsContent]);
    const figma = {fileKey:'fixture', getNodeByIdAsync:async()=>node,
      base64Encode:b=>Buffer.from(b).toString('base64'),
      getImageByHash:()=>({getBytesAsync:async()=>{ reads++; return raw; }}), ...api};
    let result;
    if (implementation === 'source') result = await new AsyncFunction('figma', source.replace('"__DEVUP_ASSET__"', JSON.stringify(options)))(figma);
    else { globalThis.figma=figma; result=await SCRIPTS.assets({asset:options}); }
    return {result, exports, reads, settings, node};
  }

  test(`${implementation}: omitted and explicit node modes preserve native crop, transforms, blur and mask routing`, async () => {
    for (const renderMode of [undefined, 'node']) {
      const {result, exports, reads, settings} = await run({renderMode, scale:2});
      assert.equal(result.status,'exported');
      assert.equal(result.renderMode,'node');
      assert.equal(exports,1); assert.equal(reads,0);
      assert.deepEqual(settings,{format:'PNG', contentsOnly:true, useAbsoluteBounds:false, constraint:{type:'SCALE',value:2}});
      assert.equal(result.data,Buffer.from(rendered).toString('base64'));
      const info=result.exportDiagnostics.renderInfo;
      assert.equal(info.scope,'exact-node-subtree');
      assert.equal(info.sourceProperties.scaleMode,'CROP');
      assert.deepEqual(info.sourceProperties.imageTransform,[[0.5,0,0.25],[0,1,0]]);
      assert.equal(info.sourceProperties.blurEffects.length,2);
      assert.equal(info.sourceProperties.isMask,true);
      assert.equal(info.sourceProperties.ancestors[0].siblingMaskPresent,true);
      assert.deepEqual(info.nodeSize,{width:100,height:80,aspectRatio:1.25});
      assert.equal(info.outputSize,null); assert.equal(info.aspectRatioComparison,'unknown');
      assert.ok(info.warnings.includes('ancestor-canvas-appearance-unverified'));
      assert.equal(info.fidelity,'unverified');
      assert.deepEqual(info.unappliedProperties,[]);
    }
  });
  test(`${implementation}: fill returns exact raw bytes and explicitly lists unapplied properties`, async () => {
    const {result,exports,reads}=await run({renderMode:'fill'});
    assert.equal(exports,0); assert.equal(reads,1);
    assert.equal(result.status,'exported'); assert.equal(result.renderMode,'fill');
    assert.equal(result.data,Buffer.from(raw).toString('base64'));
    const info=result.exportDiagnostics.renderInfo;
    assert.equal(info.representation,'original-fill');
    for (const property of ['scaleMode','imageTransform','nodeEffectsIncludingBlur','nodeAndAncestorMasks']) assert.ok(info.unappliedProperties.includes(property));
    assert.equal(info.scaleSemantics,'intrinsic-original-bytes-no-resize');
    assert.ok(info.warnings.includes('original-fill-properties-unapplied'));
  });
  test(`${implementation}: fill refuses rescale, incompatible formats, node sources, remote and unknown mode`, async () => {
    for (const [options,code] of [
      [{renderMode:'fill',scale:2},'DEVUP_ASSET_FILL_SETTINGS_UNSUPPORTED'],
      [{renderMode:'fill',format:'svg'},'DEVUP_ASSET_FILL_SETTINGS_UNSUPPORTED'],
      [{renderMode:'fill',format:'pdf'},'DEVUP_ASSET_FILL_SETTINGS_UNSUPPORTED'],
      [{renderMode:'fill',field:'node'},'DEVUP_ASSET_FILL_SETTINGS_UNSUPPORTED'],
      [{renderMode:'fill',transport:'remote'},'DEVUP_ASSET_FILL_REQUIRES_BRIDGE'],
      [{renderMode:'approximate'},'DEVUP_ASSET_RENDER_MODE_UNSUPPORTED'],
      [{renderMode:'fill',format:'jpg'},'DEVUP_ASSET_FILL_CODEC_MISMATCH'],
    ]) {
      const {result,exports}=await run(options);
      assert.equal(result.status,'failed'); assert.equal(result.errorCode,code);
      assert.equal(result.data,undefined); assert.equal(result.byteLength,null); assert.equal(exports,0);
    }
  });
  test(`${implementation}: source change and native failure never fabricate or fall back to raw bytes`, async () => {
    for (const [options,changes,code] of [
      [{imageHash:'changed'},{},'DEVUP_ASSET_SOURCE_CHANGED'],
      [{renderMode:'node'},{reject:true},'DEVUP_ASSET_EXPORT_FAILED'],
      [{renderMode:'node',format:'svg'},{reject:true},'DEVUP_ASSET_EXPORT_FAILED'],
    ]) {
      const {result,reads}=await run(options,changes);
      assert.equal(result.errorCode,code); assert.equal(result.status,'failed');
      assert.equal(result.data,undefined); assert.equal(result.sha256,null); assert.equal(reads,0);
      if (changes.reject) assert.equal(result.exportDiagnostics.renderInfo.representation,'not-exported');
    }
  });
  test(`${implementation}: unavailable source properties and dimensions remain unknown`, async () => {
    const {result}=await run({}, {width:undefined,height:undefined,effects:undefined,isMask:undefined,parent:null,
      fills:[{type:'IMAGE',imageHash:'source'}]});
    const info=result.exportDiagnostics.renderInfo;
    assert.equal(info.nodeSize,null); assert.equal(info.outputSize,null);
    assert.equal(info.sourceProperties.scaleMode,null); assert.equal(info.sourceProperties.effects,null);
    assert.equal(info.sourceProperties.isMask,null); assert.equal(info.fidelity,'unverified');
  });
  test(`${implementation}: an ancestor's current mask branch is not mislabeled a sibling mask`, async () => {
    const parent={type:'FRAME',isMask:true,children:[]};
    parent.parent={type:'FRAME',children:[parent]};
    const {result}=await run({}, {parent});
    assert.equal(result.exportDiagnostics.renderInfo.sourceProperties.ancestors[1].siblingMaskPresent,false);
  });
  test(`${implementation}: direct-leaf exports carry a bounded readable parent name path`, async () => {
    const parent={type:'FRAME',name:'Gallery',children:[],parent:{type:'FRAME',name:'예시 화면',
      parent:{type:'FRAME',name:'x'.repeat(1000),parent:{type:'PAGE',name:'Page'}}}};
    const {result}=await run({}, {name:'Rectangle 32',parent});
    const names=result.exportDiagnostics.renderInfo.layerHierarchy;
    assert.deepEqual(names,['x'.repeat(128),'예시 화면','Gallery','Rectangle 32']);
    assert.equal(names.length,4);
    assert.ok(names.every(name=>[...name].length<=128));
  });

  const safe = () => ({type:'RECTANGLE',width:100,height:80,visible:true,opacity:1,blendMode:'NORMAL',
    fills:[{type:'IMAGE',imageHash:'source',visible:true,opacity:1,blendMode:'NORMAL',scaleMode:'FILL',imageTransform:[[1,0,0],[0,1,0]],rotation:0,filters:{}}],
    effects:[],strokes:[],cornerRadius:0,isMask:false,clipsContent:undefined,
    absoluteTransform:[[1,0,10],[0,1,20]],relativeTransform:[[1,0,10],[0,1,20]],parent:{type:'PAGE',children:[]}});
  const sourceApi = (size={width:100,height:80}, bytes=raw) => ({getImageByHash:()=>({getSizeAsync:async()=>size,getBytesAsync:async()=>bytes})});
  test(`${implementation}: WebP acquisition uses native PNG at requested scale and preserves optional dimensions`, async () => {
    const {result,exports,reads,settings}=await run({format:'webp',scale:3,quality:12,lossless:false},safe(),sourceApi());
    assert.equal(exports,1); assert.equal(reads,0); assert.equal(settings.format,'PNG');
    assert.deepEqual(settings.constraint,{type:'SCALE',value:3}); assert.equal(result.mimeType,'image/png');
    assert.equal(result.format,'webp'); assert.equal(result.exportDiagnostics.renderInfo.representation,'native-node');
    assert.equal(result.exportDiagnostics.renderInfo.sourceSize.width,100);
    for (const mode of ['node','auto']) {
      const started=Date.now();
      const {result,exports}=await run({renderMode:mode},safe(),{getImageByHash:()=>({getSizeAsync:()=>new Promise(()=>{}),getBytesAsync:async()=>raw})});
      assert.equal(result.status,'exported'); assert.equal(exports,1); assert.equal(result.exportDiagnostics.renderInfo.sourceSize,null);
      assert.ok(Date.now()-started<1500,'optional source size must not consume native export timeout');
    }
    const auto=await run({renderMode:'auto'},safe(),sourceApi());
    assert.equal(auto.result.exportDiagnostics.renderInfo.sourceSize.width,100);
    assert.equal(auto.result.exportDiagnostics.renderInfo.sourceSizeEvidence,'Figma-image-getSizeAsync');
  });
  test(`${implementation}: auto selects raw only with explicit safe properties, matching dimensions and codec`, async () => {
    const {result,exports}=await run({renderMode:'auto'},safe(),sourceApi());
    assert.equal(result.renderMode,'auto'); assert.equal(result.status,'exported'); assert.equal(exports,0);
    const info=result.exportDiagnostics.renderInfo;
    assert.equal(info.renderedAs,'raw-fill'); assert.equal(info.autoDecision.eligible,true);
    assert.deepEqual(info.autoDecision.reasons,[]); assert.equal(info.effectsBakedIntoAsset,false);
    assert.equal(info.visibleSourceRatio,null); assert.equal(info.fidelity,'unverified');
  });
  test(`${implementation}: auto risky and unknown properties use the exact native node`, async () => {
    const cases = [
      {effects:[{type:'LAYER_BLUR',radius:12}]}, {effects:[{type:'DROP_SHADOW',radius:6}]},
      {effects:undefined}, {isMask:true}, {cornerRadius:4}, {opacity:0.5}, {strokes:[{type:'SOLID'}]},
      {absoluteTransform:[[1,0,0.5],[0,1,0]]},
      {parent:{type:'FRAME',effects:[],isMask:false,clipsContent:true,opacity:1,visible:true,blendMode:'NORMAL',children:[],parent:{type:'PAGE',children:[]}}},
      {parent:{type:'PAGE',children:[{isMask:true}]}}, {parent:null},
      {fills:[...safe().fills,{type:'GRADIENT_LINEAR',blendMode:'MULTIPLY'}]},
      {fills:[{...safe().fills[0],imageTransform:[[0.5,0,0.25],[0,0.8,0]]}]},
      {fills:[{...safe().fills[0],imageTransform:undefined}]},
      {fills:[{...safe().fills[0],scaleMode:'FIT'}]}, {fills:[{...safe().fills[0],scaleMode:'CROP'}]},
      {fills:[{...safe().fills[0],filters:{contrast:0.2}}]},
    ];
    for (const changes of cases) {
      const {result,exports}=await run({renderMode:'auto'},{...safe(),...changes},sourceApi());
      assert.equal(result.status,'exported',JSON.stringify(changes)); assert.equal(exports,1);
      assert.equal(result.exportDiagnostics.renderInfo.renderedAs,'node-render');
      assert.equal(result.exportDiagnostics.renderInfo.autoDecision.eligible,false);
      assert.ok(result.exportDiagnostics.renderInfo.autoDecision.reasons.length>0);
    }
    for (const [options,api] of [[{scale:2},sourceApi()],[{format:'svg'},sourceApi()],
      [{},sourceApi({width:200,height:80})],[{},sourceApi(null)],[{},sourceApi(undefined,new Uint8Array([255,216,255]))],
      [{transport:'remote'},sourceApi()],[{},{getImageByHash:()=>{throw Error('source unavailable');}}]]) {
      const {result,exports}=await run({renderMode:'auto',...options},safe(),api);
      assert.equal(result.status,'exported'); assert.equal(exports,1); assert.equal(result.exportDiagnostics.renderInfo.renderedAs,'node-render');
    }
  });
  test(`${implementation}: real override evidence distinguishes per-instance paints; missing comparison stays unknown`, async () => {
    for (const hash of ['first-image','second-image']) {
      const {result}=await run({imageHash:hash}, {fills:[{...safe().fills[0],imageHash:hash}],parent:{id:'instance-'+hash,type:'INSTANCE',overrides:[{id:'sample',overriddenFields:['fills']}],children:[]}});
      const info=result.exportDiagnostics.renderInfo;
      assert.equal(info.instanceOverride,true); assert.equal(info.instanceOverrideEvidence.kind,'figma-instance-overrides');
      assert.equal(result.imageHash,hash);
    }
    const {result}=await run({}, {parent:{type:'INSTANCE',overrides:[],children:[]}});
    assert.equal(result.exportDiagnostics.renderInfo.instanceOverride,'unknown');
  });
}
