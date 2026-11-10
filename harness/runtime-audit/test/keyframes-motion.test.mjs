// The keyframes entrance devup-mcp ships, compiled with the installed Devup UI
// and audited in a browser at a mobile and a desktop viewport.
import assert from 'node:assert/strict';
import test from 'node:test';
import {cp,mkdtemp,mkdir,readFile,readdir,writeFile,rm} from 'node:fs/promises';
import {join,resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {build,preview} from 'vite';
import {DevupUI} from '@devup-ui/vite-plugin';
import {chromium} from 'playwright';
import {runMotionCheck} from '../motion-checks.mjs';
import {REVEAL_SCENARIOS} from '../reveal-checks.mjs';

const harness=resolve(dirname(fileURLToPath(import.meta.url)),'..'),repo=resolve(harness,'../..');
const VIEWPORTS=[{name:'mobile',width:390,height:844},{name:'desktop',width:1440,height:900}];
const pixel=Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==','base64');

test('the shipped Devup UI keyframes reveal builds to static CSS and passes every reveal check at mobile and desktop', {timeout:1500000}, async()=> {
 await mkdir(join(harness,'out'),{recursive:true});const dir=await mkdtemp(join(harness,'out','keyframes-'));let server,browser;
 try {
  await cp(join(harness,'fixtures/keyframes-motion'),dir,{recursive:true});
  const templates=join(repo,'crates/devup-mcp/src/server/behavior_contracts/templates');
  for(const name of ['Reveal.tsx','MotionController.tsx','motion-runtime.ts'])await writeFile(join(dir,name),await readFile(join(templates,name)));
  await mkdir(join(dir,'public'),{recursive:true});await writeFile(join(dir,'public','pixel.png'),pixel);
  const config={root:dir,configFile:false,logLevel:'error',plugins:[DevupUI()],build:{outDir:'dist',target:'esnext'},preview:{host:'127.0.0.1',port:0}};
  await build(config);
  // Static extraction: the entrance is CSS, and no animation library ships.
  const assets=await readdir(join(dir,'dist/assets'));
  const read=async extension=>(await Promise.all(assets.filter(name=>name.endsWith(extension)).map(name=>readFile(join(dir,'dist/assets',name),'utf8')))).join('\n');
  const css=await read('.css'),js=await read('.js');
  for(const rule of ['@keyframes','animation-timeline:view()','animation-range:entry 0 entry min(100%,240px)','animation-fill-mode:both','prefers-reduced-motion:reduce',':focus-within{animation:none}','[data-reveal-state="pending"]','[data-reveal-state="in"]','calc(var(--reveal-index) * 60ms)'])assert.ok(css.includes(rule),`compiled CSS lacks ${rule}`);
  for(const library of ['framer-motion','motion/react','gsap'])assert.ok(!js.includes(library),`bundle contains ${library}`);
  server=await preview(config);browser=await chromium.launch();const url=`http://127.0.0.1:${server.httpServer.address().port}`;
  // Server markup from this same bundle, so "no JavaScript" is a real page.
  const once=await browser.newPage();await once.goto(`${url}/?prerender`);await once.waitForFunction(()=>typeof window.prerendered==='string');
  const markup=await once.evaluate(()=>window.prerendered);await once.close();
  assert.ok(markup.includes('id="reveal"')&&!markup.includes('data-reveal-state'),'server markup carries no hidden state');
  const index=join(dir,'dist/index.html');await writeFile(index,(await readFile(index,'utf8')).replace('<!--app-->',markup));

  const frames=join(dir,'frames');await mkdir(frames,{recursive:true});
  const audit=async(viewport,path,options)=> {
   const page=await browser.newPage({viewport:{width:viewport.width,height:viewport.height}}),errors=[];page.on('pageerror',error=>errors.push(String(error)));
   await page.goto(`${url}${path}`);await page.waitForFunction(()=>typeof window.stopMotion==='function');
   let shots=0;const capture=async name=>{const file=`${viewport.name}-${options.candidateId.replace(/\W+/g,'-')}-${name}.png`;await page.screenshot({path:join(frames,file)});shots++;return {path:file};};
   const report=await runMotionCheck(page,{kind:'reveal',durationMs:400,settleMs:150,...options},{capture});
   assert.deepEqual(errors,[],`${viewport.name} ${options.candidateId}`);await page.close();
   return {report,shots};
  };
  for(const viewport of VIEWPORTS) {
   const label=`${viewport.name} ${viewport.width}x${viewport.height}`;
   // The scroll-linked section: every scenario runs; the two that need a group have nothing to observe here.
   const {report:section,shots}=await audit(viewport,'/',{candidateId:'section-entrance:reveal',target:'#reveal',navigate:'#other'});
   assert.equal(section.status,'pass',`${label}: ${JSON.stringify(section)}`);
   assert.equal(section.measured.mode,'scroll-linked');assert.equal(shots,3,'start, middle and end frames were captured');
   const {start,middle,end}=section.measured.frames;
   assert.equal(start.opacity,0,label);assert.ok(start.translate>15&&start.translate<=16,label);assert.equal(start.visibleRatio,0,'hidden only while off screen');
   assert.ok(middle.opacity>0&&middle.opacity<1&&middle.translate>0&&middle.translate<16,`${label}: ${JSON.stringify(middle)}`);
   assert.equal(end.opacity,1,label);assert.equal(end.translate,0,label);
   assert.equal(section.measured.trigger.startedOffscreen,false);assert.equal(section.measured.trigger.late,false);assert.ok(section.measured.trigger.completeRatio<=1);
   assert.deepEqual(section.measured.reentry,{verified:true,replayed:false,hiddenWhileAway:false});
   assert.deepEqual(section.measured.notApplicable,['stagger','progress-sync'],label);assert.deepEqual(section.measured.unverified,[],label);assert.deepEqual(section.measured.scenariosNotRun,[]);
   assert.deepEqual(Object.entries(section.measured.scenarios).filter(([,result])=>result.status==='pass').map(([name])=>name),REVEAL_SCENARIOS.filter(name=>!['stagger','progress-sync'].includes(name)),label);
   assert.equal(section.measured.scenarios.bfcache.restoredFromBfcache,true);assert.equal(section.measured.scenarios['client-nav'].sameDocument,true);
   // The same section through the marker a browser without view timelines gets: one timed play.
   const {report:fallback}=await audit(viewport,'/?timed',{candidateId:'section-entrance:fallback',target:'#reveal',scenarios:['fast-scroll','reload','history','image-delay','image-failure','focus','reduced-motion','no-script']});
   assert.equal(fallback.status,'pass',`${label}: ${JSON.stringify(fallback)}`);assert.equal(fallback.measured.mode,'timed');
   assert.equal(fallback.measured.trigger.startedOffscreen,false);assert.equal(fallback.measured.trigger.late,false);
   assert.ok(fallback.measured.frames.middle.opacity>0&&fallback.measured.frames.middle.opacity<1,JSON.stringify(fallback.measured.frames));assert.equal(fallback.measured.frames.end.opacity,1);
   // A card group on a clock: one trigger, items 60ms apart, done well inside a second.
   const {report:group}=await audit(viewport,'/',{candidateId:'section-entrance:cards',target:'#cards',items:'.card',navigate:'#other'});
   assert.equal(group.status,'pass',`${label}: ${JSON.stringify(group)}`);assert.equal(group.measured.mode,'timed');
   const stagger=group.measured.scenarios.stagger;
   assert.equal(stagger.status,'pass',JSON.stringify(stagger));assert.equal(stagger.items,4);assert.equal(stagger.ordered,true);
   // Nominal: 60ms steps, a 180ms span, 580ms in total. Frame times vary by machine, so only the bounds and a floor are asserted.
   assert.ok(stagger.stepsMs.every(step=>step<=150),JSON.stringify(stagger));assert.ok(stagger.spanMs>=60&&stagger.spanMs<=450,JSON.stringify(stagger));assert.ok(stagger.totalMs>=400&&stagger.totalMs<=1000,JSON.stringify(stagger));
   assert.deepEqual(group.measured.notApplicable,['image-delay','image-failure','focus','progress-sync'],label);
   // A timeline: the line and its steps share one variable, so each step appears as the line reaches it.
   const {report:timeline}=await audit(viewport,'/',{candidateId:'section-entrance:timeline',target:'#timeline',items:'.step',progress:'.line',scenarios:['progress-sync','stagger','fast-scroll','reduced-motion','no-script'],maxStepMs:400,maxSpanMs:900,maxTotalMs:1400});
   assert.equal(timeline.status,'pass',`${label}: ${JSON.stringify(timeline)}`);
   const sync=timeline.measured.scenarios['progress-sync'];assert.equal(sync.axis,'vertical');assert.ok(sync.gaps.every(gap=>Math.abs(gap)<=200),JSON.stringify(sync));
  }
  assert.equal((await readdir(frames)).length,VIEWPORTS.length*4*3,'three frames per check per viewport');

  // Unmounting the controller leaves nothing hidden.
  const tidy=await browser.newPage({viewport:{width:390,height:844}});await tidy.goto(`${url}/?timed`);await tidy.waitForFunction(()=>document.querySelector('[data-reveal-state="pending"]'));
  await tidy.evaluate(()=>window.stopMotion());assert.equal(await tidy.locator('[data-reveal-state]').count(),0);await tidy.close();
 } finally {if(browser)await browser.close();if(server)await new Promise(done=>server.httpServer.close(done));if(!process.env.DEVUP_KEEP_RENDER_EVIDENCE)await rm(dir,{recursive:true,force:true});}
});
