import test from 'node:test';
import assert from 'node:assert/strict';
import {referenceForViewport,captureCalibrationIssues} from '../verify-render.mjs';
const viewport={width:1440,height:900,deviceScaleFactor:1};
const context={innerWidth:1440,innerHeight:900,devicePixelRatio:1,visualViewportScale:1,rootCssZoom:1,bodyCssZoom:1};
test('reference pixel height does not replace the CSS viewport height',()=>{
 const entry={screen:'home',width:1440,height:5953};
 assert.equal(referenceForViewport([entry],'home',viewport),entry);
 assert.deepEqual(captureCalibrationIssues(viewport,context),[]);
 assert.throws(()=>referenceForViewport([entry,entry],'home',viewport),/exactly one/);
 assert.equal(referenceForViewport([{...entry,viewportHeight:900},{...entry,viewportHeight:800}],'home',viewport).viewportHeight,900);
});
test('unproven DPR and measured zoom stay inconclusive rather than implying resizing or layout failure',()=>{
 assert.match(captureCalibrationIssues({...viewport,deviceScaleFactor:2},{...context,devicePixelRatio:2}).join(' '),/independent logical capture/);
 for(const field of ['rootCssZoom','bodyCssZoom','visualViewportScale'])assert.ok(captureCalibrationIssues(viewport,{...context,[field]:1.25}).length,field);
 assert.ok(captureCalibrationIssues(viewport,undefined).length);
});
