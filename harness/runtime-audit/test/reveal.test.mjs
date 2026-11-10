// Reveal checks against small pages that each implement the same entrance a
// different way. One page per rule: the correct one passes, and every other
// one breaks exactly one rule and must fail (or be unverified) on that rule.
import assert from 'node:assert/strict';
import test from 'node:test';
import {createServer} from 'node:http';
import {chromium} from 'playwright';
import {runMotionCheck} from '../motion-checks.mjs';
import {REVEAL_SCENARIOS} from '../reveal-checks.mjs';

const pixel=Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==','base64');
const VIEWPORTS=[{width:390,height:844},{width:1440,height:900}];
// Taller than any viewport used here plus the early page's 600px margin, so nothing is in range at load.
const spacer='<div style="height:2200px"></div>';

// A timed entrance started by a script: a 300ms fade and upward move when the
// section enters. `good` plays once, leaves content alone when it is already
// on screen, focused or when reduced motion is asked for.
const timed=(mode,bare)=>`<!doctype html><body style="margin:0">${spacer}
<section id="reveal" style="height:200px"${mode==='markup-hidden'?' data-hidden':''}>${bare?'Readable server section':'<a href="#x">Link</a><img src="/pixel.png" width="40" height="40" alt="">'}</section>
${mode==='markup-hidden'?'<style>[data-hidden]{opacity:0}</style>':''}${spacer}<a id="after" href="#y">After</a><script>
const mode=${JSON.stringify(mode)},node=document.querySelector('#reveal');let played=false,first=true;
node.removeAttribute('data-hidden');
const reduce=matchMedia('(prefers-reduced-motion: reduce)').matches&&mode!=='ignores-preference';
const play=()=>node.animate([{opacity:0,transform:'translateY(24px)'},{opacity:1,transform:'translateY(0)'}],{duration:300,easing:'linear'});
if(mode==='image-gated'){node.style.opacity='0';node.querySelector('img').addEventListener('load',()=>{node.style.opacity='';});}
if(mode==='pageshow-replay')addEventListener('pageshow',event=>{if(event.persisted)play();});
if(mode==='rehide'||mode==='replay')new IntersectionObserver(entries=>entries.forEach(entry=> {
 if(!entry.isIntersecting){if(mode==='rehide')node.style.opacity='0';return;}
 node.style.opacity='';if(played&&mode!=='replay')return;played=true;play();
})).observe(node);
else new IntersectionObserver(entries=>entries.forEach(entry=> {
 const initial=first;first=false;
 if(!entry.isIntersecting||played)return;played=true;
 if(mode==='stuck'){node.style.opacity='0';return;}
 if(mode==='late'){setTimeout(play,900);return;}
 if(initial&&mode!=='flash')return;
 if(reduce)return;
 if(node.contains(document.activeElement)&&mode!=='focusanim')return;
 play();
}),{rootMargin:mode==='early'?'600px':'0px'}).observe(node);
</script></body>`;

// The same entrance as static CSS on a view timeline. `css-good` is the
// recommended form; the others drop one declaration each.
const linked=(mode,tall)=>`<!doctype html><style>
@keyframes rise{from{opacity:0;transform:translateY(16px)}to{opacity:1;transform:translateY(0)}}
#reveal{animation:rise linear both;animation-timeline:view();${mode==='css-cover'?'':`animation-range:entry 0% entry ${mode==='css-entry'?'100%':'min(100%, 240px)'}`}}
${mode==='css-nofocus'?'':'#reveal:focus-within{animation:none}'}
${mode==='css-noreduce'?'':'@media (prefers-reduced-motion: reduce){#reveal{animation:none}}'}
</style><body style="margin:0">${spacer}
<section id="band" style="background:#eef"><div id="reveal" style="height:${tall?1200:200}px"><a href="#x" style="display:block">Link</a><img src="/pixel.png" width="40" height="40" alt=""></div></section>
${spacer}<a id="after" href="#y">After</a></body>`;

// A card group on a clock: marked once when it enters, items delayed by index.
const group=(step,duration,count)=>`<!doctype html><style>
@keyframes rise{from{opacity:0;transform:translateY(16px)}to{opacity:1;transform:translateY(0)}}
#reveal{display:flex;gap:8px;height:120px}.item{width:60px}
#reveal.pending .item{opacity:0}
#reveal.in .item{animation:rise ${duration}ms ease-out both;animation-delay:calc(var(--i) * ${step}ms)}
</style><body style="margin:0">${spacer}
<section id="reveal">${Array.from({length:count},(_,index)=>`<div class="item" style="--i:${index}">Card ${index+1}</div>`).join('')}</section>
${spacer}<script>
const node=document.querySelector('#reveal');
if(node.getBoundingClientRect().top>=innerHeight){node.className='pending';const observer=new IntersectionObserver(entries=>entries.forEach(entry=>{if(!entry.isIntersecting)return;node.className='in';observer.disconnect();}));observer.observe(node);}
</script></body>`;

// A timeline: a vertical line grows over 900ms while three steps fade in.
// `good` delays each step until the line reaches it; `bad` shows all at once.
const timeline=mode=>`<!doctype html><body style="margin:0">${spacer}
<section id="reveal" style="position:relative;height:400px"><i class="line" style="position:absolute;left:0;top:0;width:2px;height:100%;background:blue;transform-origin:top"></i>
${[0,1,2].map(index=>`<p class="step" style="position:absolute;left:20px;top:${index*133}px;margin:0;height:40px">Step ${index+1}</p>`).join('')}</section>
${spacer}<script>
const node=document.querySelector('#reveal');let played=false;
new IntersectionObserver(entries=>entries.forEach(entry=> {
 if(!entry.isIntersecting||played)return;played=true;
 node.querySelector('.line').animate([{transform:'scaleY(0)'},{transform:'scaleY(1)'}],{duration:900,easing:'linear',fill:'both'});
 node.querySelectorAll('.step').forEach((step,index)=>step.animate([{opacity:0},{opacity:1}],{duration:300,delay:${JSON.stringify(mode)}==='good'?index*300:0,easing:'linear',fill:'both'}));
})).observe(node);
</script></body>`;

// One document, two routes, like a framework router: the view is swapped,
// history gets an entry, and going back restores the view and the scroll.
const router=mode=>`<!doctype html><body style="margin:0"><nav><a id="away" href="/away">Away</a></nav><div id="view">${spacer}
<section id="reveal" style="height:200px">Readable server section</section>${spacer}</div><script>
const mode=${JSON.stringify(mode)},view=document.querySelector('#view'),home=view.innerHTML;
const play=node=>node.animate([{opacity:0,transform:'translateY(24px)'},{opacity:1,transform:'translateY(0)'}],{duration:300,easing:'linear'});
const attach=returning=>{const node=document.querySelector('#reveal');let first=true,played=false;new IntersectionObserver(entries=>entries.forEach(entry=> {
 const initial=first;first=false;if(!entry.isIntersecting||played)return;played=true;
 // Content already on screen stays as rendered, unless this page replays it after navigation.
 if(initial&&!(returning&&mode==='navflash'))return;
 play(node);
})).observe(node);};
attach(false);
document.querySelector('#away').addEventListener('click',event=> {
 if(mode==='document')return;
 event.preventDefault();history.replaceState({y:scrollY},'');history.pushState({},'','/away');view.innerHTML='<h1>Away</h1>';scrollTo(0,0);
});
addEventListener('popstate',event=>{view.innerHTML=home;scrollTo(0,event.state?.y??0);attach(true);});
</script></body>`;

async function withPages(run) {
 const server=createServer((request,response)=> {
  const url=new URL(request.url,'http://local'),mode=url.searchParams.get('mode');
  if(url.pathname==='/pixel.png'){response.writeHead(200,{'content-type':'image/png','cache-control':'no-store'});response.end(pixel);return;}
  if(url.pathname==='/__devup-bfcache-away__'||url.pathname==='/away'){response.writeHead(200,{'content-type':'text/html'});response.end('<!doctype html><title>away</title>');return;}
  const kind=url.searchParams.get('kind')??'timed';
  const body=kind==='linked'?linked(mode,url.searchParams.has('tall')):kind==='group'?group(Number(url.searchParams.get('step')),Number(url.searchParams.get('duration')),Number(url.searchParams.get('count'))):kind==='timeline'?timeline(mode):kind==='router'?router(mode):timed(mode,url.searchParams.has('bare'));
  response.writeHead(200,{'content-type':'text/html','cache-control':'no-cache'});response.end(body);
 });
 await new Promise(done=>server.listen(0,'127.0.0.1',done));
 const browser=await chromium.launch();
 try {
  const check=async(query,options={},viewport=VIEWPORTS[0],context={})=> {
   const tab=await browser.newPage({viewport});
   await tab.goto(`http://127.0.0.1:${server.address().port}/?${query}`);
   const report=await runMotionCheck(tab,{kind:'reveal',candidateId:`reveal:${query}`,target:'#reveal',durationMs:300,settleMs:150,...options},context);
   await tab.close();return report;
  };
  await run(check);
 } finally {await browser.close();await new Promise(done=>server.close(done));}
}
const statuses=report=>Object.fromEntries(Object.entries(report.measured.scenarios).map(([name,result])=>[name,result.status]));
const without=(...names)=>REVEAL_SCENARIOS.filter(name=>!names.includes(name));

test('a one-shot reveal passes at mobile and desktop; what could not be checked is unverified, never passed', {timeout:600000}, async()=> {
 await withPages(async check=> {
  for(const viewport of VIEWPORTS) {
   // Every scenario by default. This page has no client router to name, so that one scenario is unverified and the check is not a pass.
   const all=await check('mode=good',{},viewport);
   assert.equal(all.status,'inconclusive',JSON.stringify(all));
   assert.deepEqual(all.measured.unverified,['client-nav']);assert.deepEqual(all.measured.notApplicable,['stagger','progress-sync']);assert.deepEqual(all.measured.scenariosNotRun,[]);
   assert.deepEqual(statuses(all),{'fast-scroll':'pass',reload:'pass',history:'pass',bfcache:'pass','client-nav':'unverified','image-delay':'pass','image-failure':'pass',focus:'pass','reduced-motion':'pass','no-script':'pass',stagger:'not-applicable','progress-sync':'not-applicable'});
   assert.equal(all.measured.scenarios.bfcache.restoredFromBfcache,true);
   assert.equal(all.measured.mode,'timed');assert.equal(all.measured.trigger.verified,true);assert.equal(all.measured.trigger.startedOffscreen,false);assert.ok(all.measured.trigger.offscreenPx<=1);
   assert.deepEqual(all.measured.reentry,{verified:true,replayed:false,hiddenWhileAway:false});
   // Start, middle and end are measured values, not a final screenshot.
   const {start,middle,end}=all.measured.frames;
   assert.ok(start.opacity<0.5,JSON.stringify(start));assert.ok(middle.opacity>0&&middle.opacity<1&&middle.translate>0,JSON.stringify(middle));assert.equal(end.opacity,1);assert.equal(end.translate,0);
   // Scoped to what this page has: a pass, with the rest named as not run.
   const scoped=await check('mode=good',{scenarios:without('client-nav','stagger','progress-sync')},viewport);
   assert.equal(scoped.status,'pass',JSON.stringify(scoped));assert.deepEqual(scoped.measured.scenariosNotRun,['client-nav','stagger','progress-sync']);assert.deepEqual(scoped.measured.unverified,[]);
  }
  // Nothing requested: every scenario is named as not run, never implied.
  const none=await check('mode=good',{scenarios:[]});
  assert.equal(none.status,'pass',JSON.stringify(none));assert.deepEqual(none.measured.scenariosNotRun,REVEAL_SCENARIOS);
  // A block with no image and no control has nothing for those scenarios to observe: not-applicable by default, a failure when the caller claimed them.
  const bare=await check('mode=good&bare',{scenarios:['image-failure','focus']});
  assert.equal(bare.status,'fail',JSON.stringify(bare));assert.equal(bare.measured.scenarios['image-failure'].verified,false);assert.equal(bare.measured.scenarios.focus.verified,false);
  const bareDefault=await check('mode=good&bare',{scenarios:undefined});
  assert.deepEqual(bareDefault.measured.notApplicable,['image-delay','image-failure','focus','stagger','progress-sync'],JSON.stringify(statuses(bareDefault)));
  // Already on screen at load: the trigger was never observed, so this is not a pass either.
  const visible=await check('mode=good',{target:'#after',scenarios:[]});
  assert.notEqual(visible.status,'pass',JSON.stringify(visible));
 });
});

test('trigger position, replay, re-hide and an unobservable re-entry fail', {timeout:300000}, async()=> {
 await withPages(async check=> {
  const early=await check('mode=early',{scenarios:[]});
  assert.equal(early.status,'fail',JSON.stringify(early));assert.equal(early.measured.trigger.startedOffscreen,true);assert.ok(early.measured.trigger.offscreenPx>100,JSON.stringify(early.measured.trigger));
  const late=await check('mode=late',{scenarios:[]});
  assert.equal(late.status,'fail',JSON.stringify(late));assert.equal(late.measured.trigger.late,true);
  const replay=await check('mode=replay',{scenarios:[]});
  assert.equal(replay.status,'fail',JSON.stringify(replay));assert.equal(replay.measured.reentry.replayed,true);
  const rehide=await check('mode=rehide',{scenarios:[]});
  assert.equal(rehide.status,'fail',JSON.stringify(rehide));assert.equal(rehide.measured.reentry.hiddenWhileAway,true);
  // The painted section was named instead of the wrapper that moves.
  const wrong=await check('kind=linked&mode=css-good',{target:'#band',scenarios:[]});
  assert.equal(wrong.status,'fail',JSON.stringify(wrong));assert.match(wrong.measured.hint,/inner wrapper/);
 });
 // A page too short to scroll the target away proves nothing about re-entry.
 const browser=await chromium.launch();
 try {
  const page=await browser.newPage({viewport:VIEWPORTS[0]});
  await page.setContent('<body style="margin:0"><section id="reveal" style="height:200px">Readable server section</section><script>const node=document.querySelector("#reveal");new IntersectionObserver((entries,observer)=>entries.forEach(entry=>{if(!entry.isIntersecting)return;observer.disconnect();node.animate([{opacity:0,transform:"translateY(24px)"},{opacity:1,transform:"translateY(0)"}],{duration:300,easing:"linear"});})).observe(node);</script></body>');
  const short=await runMotionCheck(page,{kind:'reveal',candidateId:'reveal:short',target:'#reveal',durationMs:300,scenarios:[]});
  assert.equal(short.status,'fail',JSON.stringify(short));assert.equal(short.measured.reentry?.verified,false);
 } finally {await browser.close();}
});

test('each fresh-load scenario fails the page that breaks its rule', {timeout:600000}, async()=> {
 await withPages(async check=> {
  for(const [query,scenario] of [
   ['mode=flash','reload'],['mode=flash','history'],['mode=stuck','fast-scroll'],['mode=focusanim','focus'],['mode=image-gated','image-failure'],['mode=image-gated','image-delay'],
   // The preference is ignored; content is hidden in server markup until a script shows it; the page replays its entrance when thawed from bfcache.
   ['mode=ignores-preference','reduced-motion'],['mode=markup-hidden','no-script'],['mode=pageshow-replay','bfcache'],
  ]) {
   const report=await check(query,{scenarios:[scenario]});
   assert.equal(report.status,'fail',`${query}/${scenario}: ${JSON.stringify(report)}`);
   assert.equal(report.measured.scenarios[scenario].status,'fail',`${query}/${scenario}: ${JSON.stringify(report.measured.scenarios)}`);
  }
  const flash=await check('mode=flash',{scenarios:['reload']});assert.equal(flash.measured.scenarios.reload.flashed,true);
  const thawed=await check('mode=pageshow-replay',{scenarios:['bfcache']});
  assert.equal(thawed.measured.scenarios.bfcache.restoredFromBfcache,true);assert.equal(thawed.measured.scenarios.bfcache.flashed,true);
  // No browser with a back/forward cache to run it in: not checked, so unverified by default and a failure when claimed.
  const launchRestorable=()=>{throw new Error('full Chromium is not installed');};
  const unavailable=await check('mode=good',{scenarios:undefined},VIEWPORTS[0],{launchRestorable});
  assert.equal(unavailable.measured.scenarios.bfcache.status,'unverified',JSON.stringify(unavailable.measured.scenarios.bfcache));assert.ok(unavailable.measured.unverified.includes('bfcache'));assert.equal(unavailable.status,'inconclusive');
  assert.equal((await check('mode=good',{scenarios:['bfcache']},VIEWPORTS[0],{launchRestorable})).status,'fail');
 });
});

test('client navigation away and back must not replay content that is on screen', {timeout:300000}, async()=> {
 await withPages(async check=> {
  for(const viewport of VIEWPORTS) {
   const good=await check('kind=router&mode=good',{navigate:'#away',scenarios:['client-nav']},viewport);
   assert.equal(good.status,'pass',JSON.stringify(good));assert.equal(good.measured.scenarios['client-nav'].sameDocument,true);assert.equal(good.measured.scenarios['client-nav'].flashed,false);
   const flash=await check('kind=router&mode=navflash',{navigate:'#away',scenarios:['client-nav']},viewport);
   assert.equal(flash.status,'fail',JSON.stringify(flash));assert.equal(flash.measured.scenarios['client-nav'].flashed,true);
  }
  // A link that loads a new document is not client navigation; no link at all leaves it unverified.
  const documents=await check('kind=router&mode=document',{navigate:'#away',scenarios:undefined});
  assert.equal(documents.measured.scenarios['client-nav'].status,'not-applicable',JSON.stringify(documents.measured.scenarios['client-nav']));
  const unnamed=await check('kind=router&mode=good',{scenarios:['client-nav']});
  assert.equal(unnamed.status,'fail');assert.equal(unnamed.measured.scenarios['client-nav'].verified,false);
 });
});

test('a view-timeline reveal is measured by position: entry range, focus and reduced motion', {timeout:600000}, async()=> {
 await withPages(async check=> {
  const scenarios=without('client-nav','stagger','progress-sync');
  for(const viewport of VIEWPORTS) {
   const good=await check('kind=linked&mode=css-good',{scenarios},viewport);
   assert.equal(good.status,'pass',JSON.stringify(good));assert.equal(good.measured.mode,'scroll-linked');
   const {start,middle,end}=good.measured.frames;
   assert.equal(start.opacity,0);assert.equal(start.visibleRatio,0);assert.ok(middle.opacity>0&&middle.opacity<1,JSON.stringify(middle));assert.equal(end.opacity,1);assert.equal(end.translate,0);
   assert.deepEqual(good.measured.reentry,{verified:true,replayed:false,hiddenWhileAway:false});
   // No range: the fade spans the whole pass, so the block is still part-transparent when fully on screen.
   const cover=await check('kind=linked&mode=css-cover',{scenarios:[]},viewport);
   assert.equal(cover.status,'fail',JSON.stringify(cover));assert.equal(cover.measured.trigger.late,true);
  }
  // A section taller than the screen: with a plain `entry 100%` it is still half faded with half a screen of it showing.
  const tall=await check('kind=linked&mode=css-good&tall',{scenarios:['fast-scroll','focus','reduced-motion','no-script']});
  assert.equal(tall.status,'pass',JSON.stringify(tall));assert.ok(tall.measured.trigger.completeVisiblePx<=260,JSON.stringify(tall.measured.trigger));
  const faded=await check('kind=linked&mode=css-entry&tall',{scenarios:[]});
  assert.equal(faded.status,'fail',JSON.stringify(faded));assert.equal(faded.measured.trigger.late,true);
  // Focus is scrolled to the middle of the screen. On a short (landscape) screen that is inside the entry, so the block must be shown at once.
  const landscape={width:844,height:390};
  assert.equal((await check('kind=linked&mode=css-good&tall',{scenarios:['focus']},landscape)).status,'pass');
  const nofocus=await check('kind=linked&mode=css-nofocus&tall',{scenarios:['focus']},landscape);
  assert.equal(nofocus.status,'fail',JSON.stringify(nofocus));assert.ok(nofocus.measured.scenarios.focus.focusedControl.opacity<1,JSON.stringify(nofocus.measured.scenarios.focus));
  const noreduce=await check('kind=linked&mode=css-noreduce',{scenarios:['reduced-motion']});
  assert.equal(noreduce.status,'fail',JSON.stringify(noreduce));assert.equal(noreduce.measured.scenarios['reduced-motion'].flashed,true);
 });
});

test('a card group stagger and a progress line are bounded and synchronised', {timeout:600000}, async()=> {
 await withPages(async check=> {
  for(const viewport of VIEWPORTS) {
   const short=(await check('kind=group&step=60&duration=400&count=5',{items:'.item',durationMs:400,scenarios:['stagger']},viewport));
   assert.equal(short.status,'pass',JSON.stringify(short));
   const measured=short.measured.scenarios.stagger;
   assert.equal(measured.items,5);assert.equal(measured.ordered,true);assert.ok(measured.spanMs>=120&&measured.spanMs<=450,JSON.stringify(measured));assert.ok(measured.totalMs>=450&&measured.totalMs<=1000,JSON.stringify(measured));
   // 250ms between cards: each step and the whole span are out of bounds.
   const slow=(await check('kind=group&step=250&duration=400&count=5',{items:'.item',durationMs:400,scenarios:['stagger']},viewport)).measured.scenarios.stagger;
   assert.equal(slow.status,'fail',JSON.stringify(slow));assert.ok(slow.stepsMs.every(step=>step>150),JSON.stringify(slow));assert.ok(slow.spanMs>450);
   // A short step but a long item: the group as a whole takes too long.
   const long=(await check('kind=group&step=60&duration=1600&count=5',{items:'.item',durationMs:400,scenarios:['stagger']},viewport)).measured.scenarios.stagger;
   assert.equal(long.status,'fail',JSON.stringify(long));assert.ok(long.totalMs>1000,JSON.stringify(long));assert.ok(long.stepsMs.every(step=>step<=150));
  }
  // The caller's own bounds apply.
  const allowed=await check('kind=group&step=250&duration=400&count=3',{items:'.item',durationMs:400,scenarios:['stagger'],maxStepMs:300,maxSpanMs:600,maxTotalMs:1200});
  assert.equal(allowed.status,'pass',JSON.stringify(allowed));
  // No items named: nothing to time.
  const unnamed=await check('kind=group&step=60&duration=400&count=5',{scenarios:['stagger']});
  assert.equal(unnamed.measured.scenarios.stagger.verified,false);
  // Progress line and items: in step passes, all-at-once fails, and a block without a line (or without the selectors) observed nothing.
  const selectors={progress:'.line',items:'.step',scenarios:['progress-sync']};
  for(const viewport of VIEWPORTS) {
   const synced=(await check('kind=timeline&mode=good',selectors,viewport)).measured.scenarios['progress-sync'];
   assert.equal(synced.status,'pass',JSON.stringify(synced));assert.equal(synced.axis,'vertical');
   const unsynced=(await check('kind=timeline&mode=bad',selectors,viewport)).measured.scenarios['progress-sync'];
   assert.equal(unsynced.status,'fail',JSON.stringify(unsynced));assert.ok(unsynced.gaps[2]<-200,JSON.stringify(unsynced));
  }
  for(const report of [await check('kind=timeline&mode=good',{scenarios:['progress-sync']}),await check('mode=good',{scenarios:['progress-sync'],items:'a',progress:'img'})]) {
   assert.equal(report.status,'fail',JSON.stringify(report));assert.equal(report.measured.scenarios['progress-sync'].verified,false);
  }
 });
});
