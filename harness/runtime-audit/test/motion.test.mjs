import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdtemp,mkdir,readFile,writeFile,rm} from 'node:fs/promises';
import {join,resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {build,preview} from 'vite';
import {chromium} from 'playwright';
import {installMotionProbe,runMotionCheck, REVEAL_SCENARIOS} from '../motion-checks.mjs';

const harness=resolve(dirname(fileURLToPath(import.meta.url)),'..'),repo=resolve(harness,'../..');

test('shipped motion controllers execute counts, seamless marquee, cycling, persistent tabs and shared reveals', {timeout:90000},async()=> {
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
  const metric=Number(new URL(location.href).searchParams.get('value')??8.52);const bindings=[{kind:'marquee',target:'#marquee',track:'.track',group:'.message',pixelsPerSecond:800,respectReducedMotion:false},{kind:'count-up',target:'#count',value:metric,precision:metric===2000?0:2,unit:metric===2000?'건':'%',durationMs:600},{kind:'progress-step-cycle',target:'#steps',steps:'button',panels:'article',progress:'.bar',durationMs:500,respectReducedMotion:false},{kind:'sliding-route-highlight',target:'#tabs',links:'a',indicator:'#indicator',durationMs:500},{kind:'section-entrance',target:'.reveal',durationMs:600}];
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
   {kind:'reveal',target:'.reveal',durationMs:600,scenarios:REVEAL_SCENARIOS.filter(name=>name!=='progress-sync')},
  ]) {
   const page=await browser.newPage({viewport:{width:360,height:700}});
   await page.addInitScript(installMotionProbe,[{...options,candidateId:`${options.kind}:fixture`}]);
   await page.goto(options.urlValue===undefined?url:`${url}/?value=${options.urlValue}`);
   await page.waitForFunction(()=>document.querySelector('.track').children.length>1&&document.querySelector('#count').hasAttribute('aria-label'));
   if(options.kind==='count-up') {await page.locator(options.target).scrollIntoViewIfNeeded();await page.waitForTimeout(800);}
   const report=await runMotionCheck(page,{...options,candidateId:`${options.kind}:fixture`});assert.equal(report.status,'pass',JSON.stringify(report));
   if(options.kind==='tabs') {const back=await runMotionCheck(page,{...options,next:'#tabs a:first-of-type'});assert.equal(back.status,'pass',JSON.stringify(back));}
   if(options.kind==='reveal') {await page.evaluate(()=>{const section=document.createElement('section');section.id='newReveal';section.className='reveal';section.textContent='New route section';section.style.cssText='height:200px;margin-top:900px';document.body.append(section);});const inserted=await runMotionCheck(page,{...options,scenarios:[],target:'#newReveal'});assert.equal(inserted.status,'pass',JSON.stringify(inserted));}
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
  assert.equal(await page.locator('#count').textContent(),'8.52%');assert.equal(await page.locator('#reveal').evaluate(node=>getComputedStyle(node).opacity),'1');await noJs.close();
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

test('reveal audit requires one-shot playback: a replay, a re-hide and an unobservable re-entry all fail', {timeout:60000}, async()=> {
 const browser=await chromium.launch();
 try {
  // Each page reveals #reveal with a 300ms fade and upward move on viewport
  // entry. `mode` decides what happens when it leaves and comes back.
  const run=async(mode,body='<div style="height:1400px"></div><section id="reveal" style="height:200px">Readable server section</section><div style="height:1400px"></div>')=> {
   const page=await browser.newPage({viewport:{width:360,height:700}});
   await page.setContent(`<body style="margin:0">${body}</body>`);
   await page.evaluate(mode=> {
    const node=document.querySelector('#reveal');let played=false;
    new IntersectionObserver(entries=>entries.forEach(entry=> {
     if(!entry.isIntersecting) {if(mode==='rehide')node.style.opacity='0';return;}
     if(mode==='rehide')node.style.opacity='';
     if(played&&mode!=='replay')return;
     played=true;node.animate([{opacity:0,transform:'translateY(24px)'},{opacity:1,transform:'translateY(0)'}],{duration:300,easing:'linear'});
    })).observe(node);
   },mode);
   const report=await runMotionCheck(page,{kind:'reveal',candidateId:`reveal:${mode}`,target:'#reveal',durationMs:300});
   await page.close();return report;
  };
  const once=await run('once');
  assert.equal(once.status,'pass',JSON.stringify(once));
  assert.deepEqual(once.measured.reentry,{verified:true,replayed:false,hiddenWhileAway:false});
  const replay=await run('replay');
  assert.equal(replay.status,'fail',JSON.stringify(replay));
  assert.equal(replay.measured.reentry.replayed,true,JSON.stringify(replay));
  const rehide=await run('rehide');
  assert.equal(rehide.status,'fail',JSON.stringify(rehide));
  assert.equal(rehide.measured.reentry.hiddenWhileAway,true,JSON.stringify(rehide));
  // A page too short to scroll the target away proves nothing about re-entry.
  const short=await run('once','<section id="reveal" style="height:200px">Readable server section</section>');
  assert.equal(short.status,'fail',JSON.stringify(short));
  assert.equal(short.measured.reentry?.verified,false,JSON.stringify(short));
 } finally {await browser.close();}
});

test('reveal audit measures trigger position and the requested fresh-load scenarios, and fails what it cannot observe', {timeout:300000}, async()=> {
 const {createServer}=await import('node:http');
 const pixel=Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==','base64');
 // One page, several implementations of the same reveal. `good` plays once
 // when the section enters and leaves content alone when it is already on
 // screen or focused; every other mode breaks exactly one rule.
 const page=(mode,bare)=>`<!doctype html><body style="margin:0"><div style="height:1400px"></div>
 <section id="reveal" style="height:200px">${bare?'Readable server section':'<a href="#x">Link</a><img src="/pixel.png" width="40" height="40" alt="">'}</section>
 <div style="height:1400px"></div><script>
 const mode=${JSON.stringify(mode)},node=document.querySelector('#reveal');let played=false,first=true;
 const play=()=>node.animate([{opacity:0,transform:'translateY(24px)'},{opacity:1,transform:'translateY(0)'}],{duration:300,easing:'linear'});
 if(mode==='image-gated'){node.style.opacity='0';node.querySelector('img').addEventListener('load',()=>{node.style.opacity='';});}
 new IntersectionObserver(entries=>entries.forEach(entry=> {
  const initial=first;first=false;
  if(!entry.isIntersecting||played)return;played=true;
  if(mode==='stuck'){node.style.opacity='0';return;}
  if(mode==='late'){setTimeout(play,900);return;}
  if(initial&&mode!=='flash')return;
  if(node.contains(document.activeElement)&&mode!=='focusanim')return;
  play();
 }),{rootMargin:mode==='early'?'600px':'0px'}).observe(node);
 </script></body>`;
 // A timeline: a vertical line grows over 900ms while three steps fade in.
 // `good` delays each step until the line reaches it; `bad` shows all at once.
 const timeline=mode=>`<!doctype html><body style="margin:0"><div style="height:1400px"></div>
 <section id="reveal" style="position:relative;height:400px"><i class="line" style="position:absolute;left:0;top:0;width:2px;height:100%;background:blue;transform-origin:top"></i>
 ${[0,1,2].map(index=>`<p class="step" style="position:absolute;left:20px;top:${index*133}px;margin:0;height:40px">Step ${index+1}</p>`).join('')}</section>
 <div style="height:1400px"></div><script>
 const node=document.querySelector('#reveal');let played=false;
 new IntersectionObserver(entries=>entries.forEach(entry=> {
  if(!entry.isIntersecting||played)return;played=true;
  node.querySelector('.line').animate([{transform:'scaleY(0)'},{transform:'scaleY(1)'}],{duration:900,easing:'linear',fill:'both'});
  node.querySelectorAll('.step').forEach((step,index)=>step.animate([{opacity:0},{opacity:1}],{duration:300,delay:${JSON.stringify(mode)}==='good'?index*300:0,easing:'linear',fill:'both'}));
 })).observe(node);
 </script></body>`;
 const server=createServer((request,response)=> {
  const url=new URL(request.url,'http://local');
  if(url.pathname==='/pixel.png'){response.writeHead(200,{'content-type':'image/png','cache-control':'no-store'});response.end(pixel);return;}
  response.writeHead(200,{'content-type':'text/html','cache-control':'no-store'});response.end(url.searchParams.has('timeline')?timeline(url.searchParams.get('mode')):page(url.searchParams.get('mode'),url.searchParams.has('bare')));
 });
 await new Promise(done=>server.listen(0,'127.0.0.1',done));
 const browser=await chromium.launch();
 try {
  const run=async(mode,scenarios,bare=false,extra={})=> {
   const tab=await browser.newPage({viewport:{width:360,height:700}});
   await tab.goto(`http://127.0.0.1:${server.address().port}/?mode=${mode}${bare?'&bare':''}${extra.progress?'&timeline':''}`);
   const report=await runMotionCheck(tab,{kind:'reveal',candidateId:`reveal:${mode}`,target:'#reveal',durationMs:300,settleMs:150,scenarios,...extra});
   await tab.close();return report;
  };
  const pageScenarios=REVEAL_SCENARIOS.filter(name=>name!=='progress-sync');
  const good=await run('good',pageScenarios);
  assert.equal(good.status,'pass',JSON.stringify(good));
  assert.equal(good.measured.trigger.verified,true);assert.equal(good.measured.trigger.startedOffscreen,false,JSON.stringify(good.measured.trigger));assert.ok(good.measured.trigger.offscreenPx<=1);
  assert.deepEqual(Object.entries(good.measured.scenarios).map(([name,result])=>[name,result.status]),pageScenarios.map(name=>[name,'pass']));
  assert.deepEqual(good.measured.scenariosNotRun,['progress-sync']);
  // Progress line and items: in step passes, all-at-once fails, and a block
  // without a line (or without the selectors) observed nothing.
  const selectors={progress:'.line',items:'.step'};
  const synced=(await run('good',['progress-sync'],false,selectors)).measured.scenarios['progress-sync'];
  assert.equal(synced.status,'pass',JSON.stringify(synced));assert.equal(synced.axis,'vertical');
  const unsynced=(await run('bad',['progress-sync'],false,selectors)).measured.scenarios['progress-sync'];
  assert.equal(unsynced.status,'fail',JSON.stringify(unsynced));assert.ok(unsynced.gaps[2]<-200,JSON.stringify(unsynced));
  for(const report of [await run('good',['progress-sync']),await run('good',['progress-sync'],false,{items:'a',progress:'img'})]) {
   assert.equal(report.status,'fail',JSON.stringify(report));assert.equal(report.measured.scenarios['progress-sync'].verified,false);
  }
  // Nothing requested: every scenario is named as not run, never implied.
  const plain=await run('good',undefined);
  assert.equal(plain.status,'pass',JSON.stringify(plain));assert.deepEqual(plain.measured.scenariosNotRun,REVEAL_SCENARIOS);
  // Trigger timing.
  const early=await run('early',[]);
  assert.equal(early.status,'fail',JSON.stringify(early));assert.equal(early.measured.trigger.startedOffscreen,true);assert.ok(early.measured.trigger.offscreenPx>100,JSON.stringify(early.measured.trigger));
  const late=await run('late',[]);
  assert.equal(late.status,'fail',JSON.stringify(late));assert.equal(late.measured.trigger.late,true);
  // One broken rule per mode, caught by its own scenario.
  for(const [mode,scenario] of [['flash','reload'],['flash','history'],['stuck','fast-scroll'],['focusanim','focus'],['image-gated','image-failure'],['image-gated','image-delay']]) {
   const report=await run(mode,[scenario]);
   assert.equal(report.status,'fail',JSON.stringify(report));
   assert.equal(report.measured.scenarios[scenario].status,'fail',`${mode}/${scenario}: ${JSON.stringify(report.measured.scenarios)}`);
  }
  const flash=await run('flash',['reload']);assert.equal(flash.measured.scenarios.reload.flashed,true,JSON.stringify(flash.measured.scenarios));
  // No image and no control: the scenario observed nothing, so it fails.
  const bare=await run('good',['image-failure','focus'],true);
  assert.equal(bare.status,'fail',JSON.stringify(bare));
  assert.equal(bare.measured.scenarios['image-failure'].verified,false);assert.equal(bare.measured.scenarios.focus.verified,false);
 } finally {await browser.close();await new Promise(done=>server.close(done));}
});
