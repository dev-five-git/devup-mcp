// Motion checks observe actual rendered geometry and state, never a screenshot
// or the presence of an animation declaration. Selectors are semantic app targets.
export function installMotionProbe(configurations) {
  const records = Object.create(null), observed = new Map();
  window.__devupCountMotion = records;
  const tick = () => {
    for (const config of configurations.slice(0,128)) {
      const nodes=document.querySelectorAll(config.target);if(nodes.length!==1)continue;
      const node=nodes[0],box=node.getBoundingClientRect(),style=getComputedStyle(node);
      if(!box.width||!box.height||box.bottom<=0||box.top>=innerHeight||style.visibility==='hidden')continue;
      if(observed.get(config.candidateId)!==node){observed.set(config.candidateId,node);records[config.candidateId]=[];}
      const painted=[...node.children].find(element=>getComputedStyle(element).visibility!=='hidden')??node;
      const value=Number((painted.textContent??'').replaceAll(',','').match(/-?\d+(?:\.\d+)?/)?.[0]);
      const list=records[config.candidateId];if(!Number.isFinite(value)||list.at(-1)?.value===value)continue;
      if(list.length<256)list.push({value,width:box.width,label:node.getAttribute('aria-label')??node.textContent,fontsReady:!document.fonts||document.fonts.status==='loaded',url:location.href});
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}

// Opt-in reveal scenarios. Those not requested are listed in
// measured.scenariosNotRun so a report never implies they were checked.
export const REVEAL_SCENARIOS = ['fast-scroll','reload','history','image-delay','image-failure','focus','progress-sync'];
export async function runMotionCheck(page, options) {
  const measured = { kind: options?.kind, candidateId: options?.candidateId, samples: [] };
  try {
    if (!options || typeof options.target !== 'string' || !['marquee','count-up','steps','tabs','reveal'].includes(options.kind)) throw new Error('motion needs kind and semantic target');
    const target = page.locator(options.target), ms = options.durationMs ?? 600;
    if (!(ms >= 80 && ms <= 10000) || await target.count() !== 1) throw new Error('motion duration/target is invalid or ambiguous');
    const sample = () => target.evaluate((node, config) => {
      const rect = element => { const box=element.getBoundingClientRect();return {x:box.x,y:box.y,width:box.width,height:box.height}; };
      const list = selector => [...node.querySelectorAll(selector)];
      if(config.kind==='count-up') {
        const painted=[...node.children].find(element=>getComputedStyle(element).visibility!=='hidden')??node;
        return {...rect(node),text:painted.textContent,label:node.getAttribute('aria-label')??node.textContent,value:Number((painted.textContent??'').replaceAll(',','').match(/-?\d+(?:\.\d+)?/)?.[0])};
      }
      if(config.kind==='steps') { const steps=list(config.steps),bars=list(config.progress);const index=steps.findIndex(element=>element.getAttribute('aria-selected')==='true');return {index,selected:steps.filter(element=>element.getAttribute('aria-selected')==='true').length,fill:bars[index]?new DOMMatrixReadOnly(getComputedStyle(bars[index]).transform).a:null}; }
      if(config.kind==='tabs') { const selected=list(config.links).filter(element=>['true','page'].includes(element.getAttribute('aria-current')??element.getAttribute('aria-selected')??''));return {indicator:rect(node.querySelector(config.indicator)),selected:selected.length===1?rect(selected[0]):null,animations:node.querySelector(config.indicator).getAnimations().filter(animation=>animation.playState==='running').length}; }
      if(config.kind==='reveal') { const style=getComputedStyle(node);return {...rect(node),opacity:Number(style.opacity),translate:new DOMMatrixReadOnly(style.transform).m42}; }
      const track=node.querySelector(config.track),groups=[...track.children],animation=track.getAnimations()[0],frames=animation?.effect?.getKeyframes()??[];
      const translate=frame=>new DOMMatrixReadOnly(String(frame?.transform??'none')).m41;
      return {clip:rect(node),track:rect(track),groups:groups.map(rect),texts:groups.map(element=>element.textContent),duplicates:groups.slice(1).every(element=>element.getAttribute('aria-hidden')==='true'&&element.inert),running:animation?.playState==='running',duration:animation?.effect?.getTiming().duration,iteration:animation?.effect?.getComputedTiming().currentIteration,keyframeDelta:frames.length>1?translate(frames.at(-1))-translate(frames[0]):null,gap:Number.parseFloat(getComputedStyle(track).columnGap)||0};
    },options);
    const take = async () => { const value=await sample();measured.samples.push(value);return value; };
    const sample_ = sample;
    let valid=false;
    if(options.kind==='count-up') {
      if(!Number.isFinite(options.finalValue))throw new Error('count-up requires expected finalValue');
      await target.scrollIntoViewIfNeeded();await page.waitForTimeout(30);const first=await take();await page.waitForTimeout(ms*.25);const middle=await take();await page.waitForTimeout(ms);const last=await take();
      const lower=Math.min(0,options.finalValue),upper=Math.max(0,options.finalValue);
      const history=await page.evaluate(id=>(window.__devupCountMotion?.[id]??[]).filter(sample=>sample.url===location.href),options.candidateId);
      measured.initialEntrySamples=history;
      const partial=history.some(sample=>sample.value>=lower&&sample.value<=upper&&sample.value!==options.finalValue);
      const stable=history.filter(sample=>sample.fontsReady);
      valid=(options.finalValue===0||partial||middle.value>=lower&&middle.value<=upper&&middle.value!==options.finalValue)&&Math.abs(last.value-options.finalValue)<10**-(options.precision??0)/2&&first.label===last.label&&Math.abs(first.width-last.width)<1&&stable.every(sample=>Math.abs(sample.width-last.width)<1);
    } else if(options.kind==='marquee') {
      const first=await take();await page.waitForTimeout(60);const moving=await take();
      const seam=await target.evaluate((node,config)=> {
        const track=node.querySelector(config.track),animation=track.getAnimations()[0];if(!animation)throw new Error('no marquee animation');
        const time=animation.currentTime,state=animation.playState,ms=Number(animation.effect.getTiming().duration);animation.pause();
        const frames=[.9999,1.0001].map(phase=> {animation.currentTime=ms*phase;return {groups:[...track.children].map(group=>{const r=group.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height};})};});
        animation.currentTime=time;if(state==='running')animation.play();return frames;
      },options);
      const before={...moving,...seam[0]},after={...moving,...seam[1]};measured.samples.push(before,after);
      const clip=sample=>({left:Math.max(0,sample.clip.x),right:Math.min(page.viewportSize().width,sample.clip.x+sample.clip.width)});
      const visible=sample=>{const bounds=clip(sample);return sample.groups.filter(box=>box.x+box.width>bounds.left&&box.x<bounds.right).sort((a,b)=>a.x-b.x);};
      const covered=sample=>{
        const bounds=clip(sample),boxes=visible(sample),gap=sample.gap+1;
        return bounds.right>bounds.left&&boxes.length>0&&boxes[0].x<=bounds.left+gap&&boxes.at(-1).x+boxes.at(-1).width>=bounds.right-gap&&boxes.slice(1).every((box,index)=>box.x-boxes[index].x-boxes[index].width<=gap);
      };
      const beforeVisible=visible(before),afterVisible=visible(after);
      const near=beforeVisible.length>0&&afterVisible.length>0&&beforeVisible.every(box=>afterVisible.some(other=>Math.abs(box.x-other.x)<8&&Math.abs(box.width-other.width)<1));
      const period=first.groups[1]?.x-first.groups[0]?.x;
      const iterationDelta=moving.iteration-first.iteration;
      // Correct for completed loop periods so a legitimate wrap cannot turn
      // leftward movement into a positive displacement. Compare to the clip
      // origin to exclude motion of a containing section.
      const displacement=(moving.track.x-moving.clip.x)-(first.track.x-first.clip.x)-iterationDelta*period;
      measured.leftwardDisplacement=displacement;
      measured.seamCoverage={beforeCount:beforeVisible.length,afterCount:afterVisible.length,beforeCovered:covered(before),afterCovered:covered(after)};
      valid=first.running&&moving.running&&first.keyframeDelta<0&&moving.keyframeDelta<0&&Number.isInteger(iterationDelta)&&iterationDelta>=0&&period>0&&displacement<-.1&&before.duplicates&&before.groups.length>=2&&before.texts.every(text=>text===before.texts[0])&&near&&covered(before)&&covered(after);
    } else if(options.kind==='steps') {
      const first=await take();await page.waitForTimeout(ms*.3);const middle=await take();await page.waitForTimeout(ms*.8);const next=await take();
      valid=first.selected===1&&middle.fill>0&&middle.fill<1&&next.index!==first.index&&next.selected===1;
      if(options.manual) {await page.locator(options.manual).click();await page.waitForTimeout(20);const manual=await take();valid=valid&&manual.index===options.manualIndex&&manual.selected===1&&manual.fill<.4;}
    } else if(options.kind==='tabs') {
      const host=await target.elementHandle(),indicator=await target.locator(options.indicator).elementHandle(),first=await take();
      await page.locator(options.next).click();
      // App Router may finish navigation after click resolves. Observe the
      // animation's actual start instead of treating network latency as motion.
      const deadline=Date.now()+ms+1500;let middle;
      do {
        await page.waitForTimeout(16);middle=await take();
        if(middle.animations>0&&middle.selected&&Math.abs(middle.indicator.x-first.indicator.x)>.1&&Math.abs(middle.indicator.x-middle.selected.x)>.1)break;
      }while(Date.now()<deadline);
      await page.waitForTimeout(ms+30);const last=await take();
      const connected=await host.evaluate((node,selector)=>node===document.querySelector(selector),options.target)&&await indicator.evaluate(node=>node.isConnected);
      valid=connected&&middle.animations>0&&last.selected&&Math.abs(last.indicator.x-last.selected.x)<1&&Math.abs(last.indicator.width-last.selected.width)<1&&Math.abs(middle.indicator.x-first.indicator.x)>.1&&Math.abs(middle.indicator.x-last.indicator.x)>.1;
    } else {
      const before=await take();
      // Scroll toward the target in small steps and record how much of it was
      // on screen when the entrance began: starting while it is still off
      // screen, or only after it has been fully visible for a while, fails.
      const trigger=await target.evaluate(async node=> {
        const frame=()=>new Promise(resolve=>requestAnimationFrame(resolve));
        // Where the block sits in layout: the entrance's own translation must not count as "not yet visible".
        const ratio=()=>{const box=node.getBoundingClientRect(),shift=new DOMMatrixReadOnly(getComputedStyle(node).transform).m42,top=box.top-shift,bottom=box.bottom-shift;return Math.max(0,Math.min(bottom,innerHeight)-Math.max(top,0))/Math.min(box.height||1,innerHeight);};
        // How far outside the viewport the block's layout box is. An observer
        // reports a block whose edge touches the viewport, so 1px is not "off screen".
        const distance=()=>{const box=node.getBoundingClientRect(),shift=new DOMMatrixReadOnly(getComputedStyle(node).transform).m42;return Math.max(0,box.top-shift-innerHeight,-(box.bottom-shift));};
        const started=()=>node.getAnimations().length>0||Number(getComputedStyle(node).opacity)<1;
        if(ratio()>0)return {verified:false,reason:'target is already in view'};
        if(started())return {verified:true,startedOffscreen:true,late:false,visibleRatio:0,offscreenPx:distance()};
        const step=Math.max(8,Math.round(innerHeight*.05));let fullSince=null;
        for(let index=0;index<4000;index++) {
          const top=node.getBoundingClientRect().top,position=scrollY;scrollBy(0,top>0?step:-step);await frame();await frame();
          const visible=ratio();
          if(started())return {verified:true,startedOffscreen:distance()>1,late:false,visibleRatio:visible,offscreenPx:distance()};
          if(visible>=.99||scrollY===position) {fullSince??=performance.now();if(performance.now()-fullSince>400)return {verified:true,startedOffscreen:false,late:true,visibleRatio:visible};}
        }
        return {verified:true,startedOffscreen:false,late:true,visibleRatio:ratio()};
      });
      measured.trigger=trigger;
      if(!trigger.verified)await target.scrollIntoViewIfNeeded();
      await page.waitForTimeout(Math.min(120,ms*.25));const middle=await take();await page.waitForTimeout(ms+30);const last=await take();valid=before.opacity===1&&middle.opacity>0&&middle.opacity<1&&middle.translate>0&&last.opacity===1&&Math.abs(last.translate)<.1&&!(trigger.verified&&(trigger.startedOffscreen||trigger.late));
      // A reveal plays once. Leave the viewport and come back: the content
      // must stay shown while away (never re-hidden) and must not replay.
      const left=await target.evaluate(node=> {
        const box=node.getBoundingClientRect(),top=scrollY+box.top,limit=document.documentElement.scrollHeight-innerHeight;
        const destination=[0,limit].find(y=>top+box.height<=y||top>=y+innerHeight);
        if(destination===undefined)return false;
        scrollTo(0,destination);return true;
      });
      if(left) {
        await page.waitForTimeout(60);const away=await take();await target.scrollIntoViewIfNeeded();await page.waitForTimeout(Math.min(120,ms*.25));const reentered=await take();await page.waitForTimeout(ms+30);const settled=await take();
        const shown=sample=>sample.opacity===1&&Math.abs(sample.translate)<.1;
        measured.reentry={verified:true,replayed:!shown(reentered),hiddenWhileAway:!shown(away)};
        valid=valid&&shown(away)&&shown(reentered)&&shown(settled);
      } else {
        // Nothing was observed, so nothing is claimed: this is not a pass.
        measured.reentry={verified:false,reason:'target cannot leave the viewport at this size'};
        if(valid)return {status:'fail',reason:'Reveal re-entry could not be observed: the target never leaves the viewport at this size. Check it at a viewport where it can scroll out.',measured};
      }
      // Requested scenarios each start from a fresh load. One that cannot be
      // observed (no image, no focusable control, scroll not restored) fails:
      // an unobserved scenario is not a passed one.
      const requested=options.scenarios??[];
      measured.scenarios={};measured.scenariosNotRun=REVEAL_SCENARIOS.filter(name=>!requested.includes(name));
      const shown=sample=>sample.opacity===1&&Math.abs(sample.translate)<.1;
      const settle=options.settleMs??400,height=last.height;
      // A reload restores the scroll position, so start from the top: the target must not already be on screen when the page loads.
      const fresh=async(waitUntil='load')=>{await page.evaluate(()=>scrollTo(0,0));await page.reload({waitUntil});await page.waitForTimeout(settle);};
      // Watches the target for a while and reports whether it was ever hidden
      // or displaced while on screen.
      const watch=()=>target.evaluate(async (node,ms)=> {
        const box=node.getBoundingClientRect();if(box.bottom<=0||box.top>=innerHeight)return {verified:false,reason:'scroll position was not restored onto the target'};
        const end=performance.now()+ms;let minOpacity=1,maxShift=0;
        while(performance.now()<end) {const style=getComputedStyle(node);minOpacity=Math.min(minOpacity,Number(style.opacity));maxShift=Math.max(maxShift,Math.abs(new DOMMatrixReadOnly(style.transform).m42));await new Promise(resolve=>requestAnimationFrame(resolve));}
        return {verified:true,minOpacity,maxShift,flashed:minOpacity<1||maxShift>.1};
      },ms+600);
      for(const name of requested) {
        let result;
        try {
          if(name==='fast-scroll') {
            await fresh();
            await page.evaluate(async()=>{for(let index=0;index<400&&scrollY<document.documentElement.scrollHeight-innerHeight-1;index++){scrollBy(0,innerHeight*1.5);await new Promise(resolve=>requestAnimationFrame(resolve));}});
            await target.evaluate(node=>node.scrollIntoView({block:'center'}));await page.waitForTimeout(ms+150);
            const sample=await sample_();result={status:shown(sample)?'pass':'fail',sample};
          } else if(name==='reload'||name==='history') {
            await fresh();await target.evaluate(node=>node.scrollIntoView({block:'center'}));await page.waitForTimeout(ms+150);
            if(name==='reload')await page.reload({waitUntil:'load'});
            else {const url=page.url();await page.goto('about:blank');await page.goBack({waitUntil:'load'});if(page.url()!==url)throw new Error('back navigation did not return to the audited URL');}
            const observed=await watch();result={status:observed.verified&&!observed.flashed?'pass':'fail',...observed};
          } else if(name==='image-delay'||name==='image-failure') {
            const images=await target.locator('img').count();
            if(!images)result={status:'fail',verified:false,reason:'target contains no image'};
            else {
              const handler=async route=>{if(route.request().resourceType()!=='image')return route.continue();if(name==='image-failure')return route.abort();await new Promise(resolve=>setTimeout(resolve,ms+1500));return route.continue().catch(()=>{});};
              await page.route('**/*',handler);
              try {await fresh('domcontentloaded');await target.evaluate(node=>node.scrollIntoView({block:'center'}));await page.waitForTimeout(ms+150);const sample=await sample_();result={status:shown(sample)&&Math.abs(sample.height-height)<1?'pass':'fail',sample,reservedHeight:height};}
              finally {await page.unroute('**/*',handler);}
            }
          } else if(name==='focus') {
            await fresh();
            const focused=await target.evaluate(async (node,selector)=> {
              const control=node.querySelector(selector);if(!control)return false;
              control.focus();for(let index=0;index<3;index++)await new Promise(resolve=>requestAnimationFrame(resolve));
              return document.activeElement===control;
            },options.focus??'a[href],button,input,select,textarea,[tabindex]');
            if(!focused)result={status:'fail',verified:false,reason:'target has no focusable control that takes focus'};
            else {await page.waitForTimeout(50);const sample=await sample_();result={status:shown(sample)?'pass':'fail',sample};}
          } else if(name==='progress-sync') {
            // A progress line and the items it connects share one timeline:
            // each item appears as the line reaches it, within a tolerance.
            if(typeof options.progress!=='string'||typeof options.items!=='string')result={status:'fail',verified:false,reason:'progress-sync needs progress and items selectors'};
            else {
              await fresh();
              const tolerance=options.syncToleranceMs??200;
              const observed=await target.evaluate(async (node,config)=> {
                const line=node.querySelector(config.progress),items=[...node.querySelectorAll(config.items)];
                if(!line||items.length<2)return {verified:false,reason:'progress line or at least two items not found'};
                node.scrollIntoView({block:'center'});
                const start=performance.now(),shownAt=items.map(()=>null),reachedAt=items.map(()=>null);let first=null,vertical=null;
                while(performance.now()-start<config.ms) {
                  const box=line.getBoundingClientRect(),now=performance.now()-start;first??={width:box.width,height:box.height};
                  if(vertical===null&&(Math.abs(box.height-first.height)>1||Math.abs(box.width-first.width)>1))vertical=Math.abs(box.height-first.height)>=Math.abs(box.width-first.width);
                  items.forEach((item,index)=> {
                    const rect=item.getBoundingClientRect();
                    // Server markup is visible until the entrance hides it, so the time that counts is the last rise above half opacity.
                    const opacity=Number(getComputedStyle(item).opacity);if(opacity<.5)shownAt[index]=null;else if(shownAt[index]===null)shownAt[index]=now;
                    if(reachedAt[index]===null&&vertical!==null&&(vertical?box.bottom>=rect.top-1:box.right>=rect.left-1))reachedAt[index]=now;
                  });
                  await new Promise(resolve=>requestAnimationFrame(resolve));
                }
                if(vertical===null)return {verified:false,reason:'progress line never advanced'};
                return {verified:true,axis:vertical?'vertical':'horizontal',shownAt,reachedAt,gaps:items.map((_,index)=>shownAt[index]===null||reachedAt[index]===null?null:shownAt[index]-reachedAt[index])};
              },{progress:options.progress,items:options.items,ms:options.progressMs??ms*3+600});
              result={status:observed.verified&&observed.gaps.every(gap=>gap!==null&&Math.abs(gap)<=tolerance)?'pass':'fail',toleranceMs:tolerance,...observed};
            }
          } else result={status:'fail',reason:'unknown scenario'};
        } catch(error) {result={status:'fail',reason:String(error.message??error)};}
        measured.scenarios[name]=result;
      }
      const failed=Object.entries(measured.scenarios).filter(([,result])=>result.status!=='pass').map(([name])=>name);
      if(failed.length)return {status:'fail',reason:`Reveal scenario failed or could not be observed: ${failed.join(', ')}`,measured};
    }
    if(options.resizeWidth) {
      await page.setViewportSize({...page.viewportSize(),width:options.resizeWidth});await page.waitForTimeout(ms+30);const resized=await take();
      if(options.kind==='tabs')valid=valid&&resized.selected&&Math.abs(resized.indicator.x-resized.selected.x)<1;
      if(options.kind==='steps')valid=valid&&resized.selected===1;
      if(options.kind==='count-up')valid=valid&&Math.abs(resized.value-options.finalValue)<10**-(options.precision??0)/2;
    }
    return {status:valid?'pass':'fail',reason:valid?'Observed intermediate motion, final state and requested continuity checks':'Motion lacked intermediate geometry, continuity or expected final state',measured};
  } catch(error) {return {status:'fail',reason:String(error.message??error),measured};}
}
