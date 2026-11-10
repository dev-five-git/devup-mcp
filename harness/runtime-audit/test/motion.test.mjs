import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdtemp,mkdir,readFile,writeFile,rm} from 'node:fs/promises';
import {join,resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {build,preview} from 'vite';
import {chromium} from 'playwright';
import {installMotionProbe,runMotionCheck} from '../motion-checks.mjs';

const harness=resolve(dirname(fileURLToPath(import.meta.url)),'..'),repo=resolve(harness,'../..');

// Entrances are static CSS now: keyframes-motion.test.mjs compiles and audits
// them, and reveal.test.mjs holds the pages that must fail.
test('shipped motion controllers execute counts, seamless marquee, cycling and persistent tabs', {timeout:90000},async()=> {
 await mkdir(join(harness,'out'),{recursive:true});const dir=await mkdtemp(join(harness,'out','motion-'));let server,browser;
 try {
  const templates=join(repo,'crates/devup-mcp/src/server/behavior_contracts/templates');
  for(const name of ['MotionController.tsx','motion-runtime.ts'])await writeFile(join(dir,name),await readFile(join(templates,name)));
  const html=`<html><head><meta charset="utf-8"><style>
  *{box-sizing:border-box}body{margin:0}#tabs{position:relative;display:flex;width:100%;height:50px}#tabs a{position:relative;width:120px;padding:15px}#tabs a:last-child{width:200px}#indicator{position:absolute;left:0;top:0;height:50px;background:#dceaff;border-radius:25px}
  #steps{display:flex}#steps button{width:100px;height:50px}#steps button[aria-selected=true]{color:blue}.bar{display:block;width:100px;height:3px;background:blue}#marquee{width:320px;max-width:100%;overflow:hidden}.track{display:flex;gap:12px}.message{width:600px;white-space:nowrap}#count{font:32px sans-serif}#reveal{height:200px}
  </style></head><body>
  <nav id="tabs"><span id="indicator"></span><a href="/one" aria-current="page">One</a><a href="/two">Two</a></nav>
  <section id="steps"><button aria-selected="true">Step one<span class="bar"></span></button><button aria-selected="false">Step two<span class="bar"></span></button><button aria-selected="false">Step three<span class="bar"></span></button><article>One panel</article><article hidden>Two panel</article><article hidden>Three panel</article></section>
  <div id="marquee"><div class="track"><div class="message">Regenerating Joints · Restoring Lives</div></div></div>
  <div style="height:950px"></div><span id="count">8.52%</span><div style="height:850px"></div><section id="reveal" class="reveal">Readable server section <a href="/one">Link</a><img src="/pixel.png" width="40" height="40" alt=""></section><div style="height:900px"></div><div id="controller"></div><script type="module" src="/main.tsx"></script></body></html>`;
  await writeFile(join(dir,'index.html'),html);await mkdir(join(dir,'public'),{recursive:true});await writeFile(join(dir,'public','pixel.png'),Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==','base64'));
  await writeFile(join(dir,'main.tsx'),`import {createElement} from 'react';import {createRoot} from 'react-dom/client';import {MotionController} from './MotionController';
  const metric=Number(new URL(location.href).searchParams.get('value')??8.52);const bindings=[{kind:'marquee',target:'#marquee',track:'.track',group:'.message',pixelsPerSecond:800,respectReducedMotion:false},{kind:'count-up',target:'#count',value:metric,precision:metric===2000?0:2,unit:metric===2000?'건':'%',durationMs:600},{kind:'progress-step-cycle',target:'#steps',steps:'button',panels:'article',progress:'.bar',durationMs:500,respectReducedMotion:false},{kind:'sliding-route-highlight',target:'#tabs',links:'a',indicator:'#indicator',durationMs:500}];
  const root=createRoot(document.querySelector('#controller'));root.render(createElement(MotionController,{bindings}));window.stopMotion=()=>root.unmount();
  document.querySelectorAll('#tabs a').forEach(link=>link.addEventListener('click',event=>{event.preventDefault();history.pushState({},'',link.getAttribute('href'));document.querySelectorAll('#tabs a').forEach(other=>other===link?other.setAttribute('aria-current','page'):other.removeAttribute('aria-current'));}));`);
  const config={root:dir,configFile:false,logLevel:'error',build:{outDir:'dist'},preview:{host:'127.0.0.1',port:0}};
  await build(config);server=await preview(config);browser=await chromium.launch();const url=`http://127.0.0.1:${server.httpServer.address().port}`;
  for(const options of [
   {kind:'marquee',target:'#marquee',track:'.track',durationMs:600},
   {kind:'count-up',target:'#count',finalValue:8.52,precision:2,durationMs:600,resizeWidth:700},
   {kind:'count-up',target:'#count',finalValue:2000,precision:0,durationMs:600,urlValue:2000},
   {kind:'count-up',target:'#count',finalValue:-8.52,precision:2,durationMs:600,urlValue:-8.52},
   {kind:'steps',target:'#steps',steps:'button',progress:'.bar',manual:'#steps button:nth-of-type(3)',manualIndex:2,durationMs:500,resizeWidth:700},
   {kind:'tabs',target:'#tabs',links:'a',indicator:'#indicator',next:'#tabs a:last-child',durationMs:500,resizeWidth:700},
  ]) {
   const page=await browser.newPage({viewport:{width:360,height:700}});
   await page.addInitScript(installMotionProbe,[{...options,candidateId:`${options.kind}:fixture`}]);
   await page.goto(options.urlValue===undefined?url:`${url}/?value=${options.urlValue}`);
   await page.waitForFunction(()=>document.querySelector('.track').children.length>1&&document.querySelector('#count').hasAttribute('aria-label'));
   if(options.kind==='count-up') {await page.locator(options.target).scrollIntoViewIfNeeded();await page.waitForTimeout(800);}
   const report=await runMotionCheck(page,{...options,candidateId:`${options.kind}:fixture`});assert.equal(report.status,'pass',JSON.stringify(report));
   if(options.kind==='tabs') {const back=await runMotionCheck(page,{...options,next:'#tabs a:first-of-type'});assert.equal(back.status,'pass',JSON.stringify(back));}
   await page.evaluate(()=>window.stopMotion());assert.equal(await page.locator('.track > *').count(),1);assert.equal(await page.evaluate(()=>document.getAnimations().filter(a=>a.playState==='running').length),0);
   await page.close();
  }
  const clock=await browser.newPage();
  await clock.addInitScript(()=>{
   const original=performance.now.bind(performance);Object.defineProperty(performance,'now',{value:()=>original()+100});window.stepSamples=[];
   const sample=()=>{const steps=[...document.querySelectorAll('#steps button')],index=steps.findIndex(node=>node.getAttribute('aria-selected')==='true');if(index>=0&&window.stepSamples.length<200)window.stepSamples.push({index,fill:new DOMMatrixReadOnly(getComputedStyle(steps[index].querySelector('.bar')).transform).a});requestAnimationFrame(sample);};requestAnimationFrame(sample);
  });
  await clock.goto(url);await clock.waitForFunction(()=>new Set(window.stepSamples.map(sample=>sample.index)).size>1);
  assert.ok(await clock.evaluate(()=>window.stepSamples.every(sample=>sample.fill>=0&&sample.fill<=1)),JSON.stringify(await clock.evaluate(()=>window.stepSamples)));
  await clock.close();

  const dynamic=await browser.newPage(),dynamicErrors=[];dynamic.on('pageerror',error=>dynamicErrors.push(String(error)));await dynamic.goto(url);await dynamic.waitForFunction(()=>document.querySelector('.track').children.length>1);
  await dynamic.locator('#steps button').nth(2).click();await dynamic.waitForTimeout(100);
  const beforeText=await dynamic.locator('#steps .bar').nth(2).evaluate(node=>new DOMMatrixReadOnly(getComputedStyle(node).transform).a);
  await dynamic.locator('#steps article').nth(2).evaluate(node=>{node.textContent='Updated API panel text';});await dynamic.waitForTimeout(50);
  const afterText=await dynamic.locator('#steps .bar').nth(2).evaluate(node=>new DOMMatrixReadOnly(getComputedStyle(node).transform).a);
  assert.ok(afterText>=beforeText,JSON.stringify({beforeText,afterText}));
  await dynamic.locator('#steps').evaluate(host=>{window.selectedStep=host.querySelector('[aria-selected="true"]');const step=document.createElement('button');step.textContent='Added API step';step.setAttribute('aria-selected','false');const bar=document.createElement('span');bar.className='bar';step.append(bar);const panel=document.createElement('article');panel.textContent='Added API panel';panel.hidden=true;host.append(step,panel);});
  await dynamic.waitForTimeout(30);assert.equal(await dynamic.evaluate(()=>window.selectedStep.getAttribute('aria-selected')),'true');
  await dynamic.getByRole('button',{name:'Added API step'}).click();assert.equal(await dynamic.getByRole('button',{name:'Added API step'}).getAttribute('aria-selected'),'true');assert.equal(await dynamic.getByText('Added API panel',{exact:true}).isVisible(),true);
  await dynamic.locator('#steps').evaluate(host=>{host.querySelector('button').remove();host.querySelector('article').remove();});await dynamic.waitForTimeout(30);
  await dynamic.getByRole('button',{name:'Added API step'}).click();await dynamic.waitForTimeout(600);
  assert.equal(await dynamic.locator('#steps button[aria-selected="true"]').count(),1);assert.equal(await dynamic.locator('#steps button').first().getAttribute('aria-selected'),'true');
  await dynamic.locator('#steps').evaluate(host=>{window.detachedSteps=host;host.remove();});await dynamic.waitForTimeout(30);
  const originalWidth=await dynamic.locator('.track > .message').first().evaluate(node=>node.getBoundingClientRect().width);
  await dynamic.locator('.track > .message').first().evaluate(node=>{node.textContent='Updated API marquee content';});
  await dynamic.waitForFunction(()=>[...document.querySelector('.track').children].every(node=>node.textContent==='Updated API marquee content'));
  assert.equal(await dynamic.locator('.track > .message').first().evaluate(node=>node.getBoundingClientRect().width),originalWidth);
  await dynamic.locator('.track > .message').first().evaluate(node=>{const replacement=node.cloneNode(true);replacement.textContent='Replacement API source group';node.replaceWith(replacement);});
  await dynamic.waitForFunction(()=>[...document.querySelector('.track').children].length>1&&[...document.querySelector('.track').children].every(node=>node.textContent==='Replacement API source group'));
  await dynamic.locator('.track').evaluate(track=>{const replacement=track.cloneNode(false),source=[...track.children].find(node=>!node.inert);const group=source.cloneNode(true);group.textContent='Replacement API track';replacement.append(group);track.replaceWith(replacement);});
  await dynamic.waitForFunction(()=>[...document.querySelector('.track').children].length>1&&[...document.querySelector('.track').children].every(node=>node.textContent==='Replacement API track'));
  await dynamic.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  await dynamic.evaluate(()=>{window.marqueeAnimation=document.querySelector('.track').getAnimations()[0];document.querySelector('#tabs a').textContent='Unrelated navigation mutation';});await dynamic.waitForTimeout(40);
  assert.equal(await dynamic.evaluate(()=>window.marqueeAnimation===document.querySelector('.track').getAnimations()[0]),true);
  await dynamic.evaluate(()=>document.querySelector('#tabs').after(window.detachedSteps));await dynamic.waitForTimeout(30);
  await dynamic.getByRole('button',{name:'Added API step'}).click();assert.equal(await dynamic.getByRole('button',{name:'Added API step'}).getAttribute('aria-selected'),'true');assert.deepEqual(dynamicErrors,[]);
  await dynamic.evaluate(()=>window.stopMotion());assert.equal(await dynamic.locator('.track > *').count(),1);await dynamic.close();

  const noJs=await browser.newContext({javaScriptEnabled:false});const page=await noJs.newPage();await page.goto(url);
  assert.equal(await page.locator('#count').textContent(),'8.52%');await noJs.close();
  const broken=await browser.newPage();await broken.setContent('<span id="static">8.52%</span>');
  const negative=await runMotionCheck(broken,{kind:'count-up',target:'#static',finalValue:8.52,precision:2,durationMs:100});assert.equal(negative.status,'fail');await broken.close();
 } finally {if(browser)await browser.close();if(server)await new Promise(done=>server.httpServer.close(done));await rm(dir,{recursive:true,force:true});}
});

test('marquee audit rejects reversed motion and an empty leftward loop seam', async()=> {
 const browser=await chromium.launch();
 try {
  for(const [name,end] of [['reversed',612],['empty-seam',-1600]]) {
   const page=await browser.newPage({viewport:{width:360,height:700}});
   await page.setContent('<div id="marquee" style="width:320px;overflow:hidden"><div class="track" style="display:flex;gap:12px;width:max-content"><div style="width:600px;flex-shrink:0">Repeated message</div><div aria-hidden="true" inert style="width:600px;flex-shrink:0">Repeated message</div></div></div>');
   await page.locator('.track').evaluate((node,end)=>node.animate([{transform:'translateX(0)'},{transform:`translateX(${end}px)`}],{duration:800,iterations:Infinity,easing:'linear'}),end);
   const report=await runMotionCheck(page,{kind:'marquee',candidateId:`marquee:${name}`,target:'#marquee',track:'.track',durationMs:800});
   assert.equal(report.status,'fail',JSON.stringify(report));
   assert.equal(report.measured.samples[0].running,true);
   if(name==='reversed')assert.ok(report.measured.samples[0].keyframeDelta>0,JSON.stringify(report));
   else {
    assert.ok(report.measured.samples[0].keyframeDelta<0,JSON.stringify(report));
    assert.equal(report.measured.seamCoverage.beforeCount,0,JSON.stringify(report));
    assert.equal(report.measured.seamCoverage.beforeCovered,false,JSON.stringify(report));
   }
   await page.close();
  }
 } finally {await browser.close();}
});
