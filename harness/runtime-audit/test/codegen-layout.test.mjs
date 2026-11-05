import assert from 'node:assert/strict';
import test from 'node:test';
import {preparedBinaries,renderFixture} from '../prepared-binaries.mjs';
const binaries=await preparedBinaries();
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdir, mkdtemp, writeFile, readFile, rm} from 'node:fs/promises';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {build, preview} from 'vite';
import {DevupUI} from '@devup-ui/vite-plugin';
import {chromium} from 'playwright';

const harness = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repo = resolve(harness, '../..');
async function rendered(snapshot, verify, { responsive = false, assets = [], reset = false } = {}) {
  await mkdir(join(harness,'out'),{recursive:true});
  const dir = await mkdtemp(join(harness,'out','layout-'));
  let server, browser;
  try {
    if (typeof snapshot !== 'string') {await writeFile(join(dir,'snapshot.json'),JSON.stringify(snapshot));snapshot=join(dir,'snapshot.json');}
    const generated = await renderFixture(snapshot,join(dir,'Generated.tsx'),{responsive});
    assert.equal(generated.status,0,generated.stderr);
    for (const asset of assets) { const target=join(dir,'public',asset.path);await mkdir(dirname(target),{recursive:true});await writeFile(target,await readFile(asset.source)); }
    await writeFile(join(dir,'devup.json'),JSON.stringify({theme:{}}));
    await writeFile(join(dir,'index.html'),'<html><body style="margin:0"><div id="root"></div><script type="module" src="/main.tsx"></script></body></html>');
    await writeFile(join(dir,'main.tsx'),`${reset?"import {resetCss} from '@devup-ui/reset-css';resetCss();":''}import {createRoot} from 'react-dom/client';import {RenderFixture} from './Generated';createRoot(document.getElementById('root')).render(<RenderFixture/>);`);
    const config={root:dir,configFile:false,logLevel:'error',plugins:[DevupUI()],build:{outDir:'dist'},preview:{host:'127.0.0.1',port:0}};
    await build(config); server=await preview(config); browser=await chromium.launch();
    await verify(browser,`http://127.0.0.1:${server.httpServer.address().port}`,dir);
  } finally {
    if(browser) await browser.close();
    if(server) await new Promise((done)=>server.httpServer.close(done));
    if(!process.env.DEVUP_KEEP_RENDER_EVIDENCE) await rm(dir,{recursive:true,force:true});
  }
}

test('Prime caller allocation and absolute marker survive compilation and match the native Figma marker', {timeout:360000}, async()=> {
 const reference=join(harness,'fixtures/prime-timeline-marker-reference.png');
 const provenance=JSON.parse(await readFile(join(harness,'fixtures/prime-timeline-marker-reference.json'),'utf8'));
 assert.equal(createHash('sha256').update(await readFile(reference)).digest('hex'),provenance.cropSha256);
 await rendered(join(repo,'crates/devup-mcp-devup-ui/tests/fixtures/prime-placement.json'),async(browser,url,dir)=> {
  const page=await browser.newPage({viewport:{width:1440,height:900},deviceScaleFactor:1});
  await page.goto(url);await page.evaluate(()=>document.fonts.ready);
  const title=page.locator('[data-figma-node-id="title"]');
  const card=page.locator('[data-figma-node-id="card"]');
  const marker=page.locator('[data-figma-node-id="marker"]');
  for(const width of [1440,980,1440]) {
   await page.setViewportSize({width,height:900});
   const boxes=await Promise.all([title,card,marker].map(element=>element.boundingBox()));
   assert.deepEqual(boxes.map(({width,height})=>({width,height})),[{width:420,height:94},{width:520,height:218},{width:16,height:16}]);
   assert.equal(boxes[1].x-boxes[0].x-boxes[0].width,40);
   assert.equal(boxes[2].x-boxes[1].x,-48);
  }
  const actual=join(dir,'prime-marker-actual.png'),diff=join(dir,'prime-marker-diff.png');
  const comparisonArgs=['--max-changed-ratio',String(provenance.comparison.maxChangedRatio),'--channel-tolerance',String(provenance.comparison.channelTolerance)];
  await marker.screenshot({path:actual});
  const compared=spawnSync(binaries.visual,['compare','--reference',reference,'--actual',actual,'--diff',diff,...comparisonArgs],{encoding:'utf8',windowsHide:true});
  assert.equal(compared.status,0,compared.stdout+compared.stderr);
  await marker.evaluate(element=>element.style.backgroundColor='#00ff00');
  const wrongColor=join(dir,'prime-marker-wrong-color.png');
  await marker.screenshot({path:wrongColor});
  const wrong=spawnSync(binaries.visual,['compare','--reference',reference,'--actual',wrongColor,...comparisonArgs],{encoding:'utf8',windowsHide:true});
  assert.equal(wrong.status,1,'wrong marker paint must fail the same Figma comparison');
  await marker.evaluate(element=>{element.style.backgroundColor='#513cee';element.style.borderRadius='0';});
  const wrongShape=join(dir,'prime-marker-wrong-shape.png');
  await marker.screenshot({path:wrongShape});
  const square=spawnSync(binaries.visual,['compare','--reference',reference,'--actual',wrongShape,...comparisonArgs],{encoding:'utf8',windowsHide:true});
  assert.equal(square.status,1,'wrong shape must fail despite the edge antialiasing tolerance');
  await writeFile(join(dir,'prime-marker-visual-report.json'),JSON.stringify({
   provenance,paths:{reference,actual,diff,wrongColor,wrongShape},
   actualSha256:createHash('sha256').update(await readFile(actual)).digest('hex'),
   correct:JSON.parse(compared.stdout),wrongColor:JSON.parse(wrong.stdout),wrongShape:JSON.parse(square.stdout),
  },null,2));
  await page.close();
 },{reset:true});
});

test('generated staggered grid keeps drawn geometry and reads 01 02 03 04 at 390', {timeout:360000}, async()=> {
  await rendered(join(repo,'crates/devup-mcp-devup-ui/tests/fixtures/staggered-sequence.json'), async(browser,url)=> {
    for(const width of [390,1100]) {
      const page=await browser.newPage({viewport:{width,height:900}});
      await page.goto(url);
      await page.locator('[data-figma-node-id="card4"]').waitFor();
      const cards=await Promise.all([1,2,3,4].map((n)=>page.locator(`[data-figma-node-id="card${n}"]`).boundingBox()));
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth),width);
      if(width===390) {
        assert.ok(cards.every((box)=>box.width<=390));
        assert.ok(cards.slice(1).every((box,i)=>box.y>cards[i].y));
      } else {
        for(const [i,[x,y]] of [[0,0],[600,60],[0,200],[600,260]].entries()) {
          assert.equal(cards[i].x,x); assert.equal(cards[i].y,y); assert.equal(cards[i].width,500); assert.equal(cards[i].height,160);
        }
      }
      await page.close();
    }
  });
});


test('generated 160px tech items, six 180px columns and three cards fit 390 and retain drawn geometry', {timeout:360000},async()=> {
  for(const [count,itemWidth] of [[6,160],[6,180],[3,350]]) {
    const snapshot=JSON.parse(await readFile(join(repo,'crates/devup-mcp-devup-ui/tests/fixtures/staggered-sequence.json'),'utf8'));
    const drawn=count*itemWidth+(count-1)*20;
    const root=snapshot.nodes.root;
    snapshot.nodes={root};
    root.fields.childrenIds=Array.from({length:count},(_,i)=>`item${i}`);
    Object.assign(root.fields,{width:drawn,height:140,layoutSizingVertical:'FIXED',itemSpacing:20,inferredAutoLayout:{layoutMode:'HORIZONTAL',itemSpacing:20},absoluteBoundingBox:{x:0,y:0,width:drawn,height:140}});
    for(const id of root.fields.childrenIds) snapshot.nodes[id]={id,type:'FRAME',fields:{parentId:'root',width:itemWidth,height:100,layoutSizingHorizontal:'FIXED',layoutSizingVertical:'FIXED',maxWidth:null,maxHeight:null}};
    await rendered(snapshot,async(browser,url)=> {
      for(const width of [390,drawn]) {
        const page=await browser.newPage({viewport:{width,height:900}});
        await page.goto(url);
        await page.locator(`[data-figma-node-id="item${count-1}"]`).waitFor();
        const items=await Promise.all(root.fields.childrenIds.map((id)=>page.locator(`[data-figma-node-id="${id}"]`).boundingBox()));
        assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth),width);
        assert.ok(items.every((box)=>box.x+box.width<=width+0.5));
        if(width===drawn) assert.equal((await page.locator('[data-figma-node-id="root"]').boundingBox()).height,140);
        if(width===drawn) for(const [i,box] of items.entries()) {
          assert.equal(box.x,i*(itemWidth+20)); assert.equal(box.y,0); assert.equal(box.width,itemWidth);
        }
        else assert.ok(items.at(-1).y>items[0].y);
        await page.close();
      }
    });
  }
});


test('captured horizontal image ratio retains its transferred minimum through the full responsive module', {timeout:360000}, async()=> {
 await rendered(join(repo,'crates/devup-mcp-devup-ui/tests/fixtures/transferred-image-minimum.json'),async(browser,url)=> {
  for(const width of [360,1440]) {
   const page=await browser.newPage({viewport:{width,height:900}});await page.goto(url);
   const image=page.locator(`[data-figma-node-ids~="${width}:image-${width===360?'mobile':'desktop'}"]`);
   await image.waitFor();await image.evaluate(e=>e.decode());
   const actual=await image.evaluate(e=>({width:e.getBoundingClientRect().width,height:e.getBoundingClientRect().height,minWidth:getComputedStyle(e).minWidth,naturalWidth:e.naturalWidth}));
   const steps=await page.locator(`[data-figma-node-ids~="${width}:steps-${width===360?'mobile':'desktop'}"]`).boundingBox();
   assert.equal(actual.naturalWidth,800);
   assert.equal(actual.minWidth,width===360?'0px':'auto');
   assert.ok(Math.abs(actual.width-(width===360?320:541))<0.1,JSON.stringify({width,actual,steps}));
   assert.ok(Math.abs(steps.width-(width===360?320:493))<0.1,JSON.stringify({width,actual,steps}));
   await page.close();
  }
 }, {responsive:true,assets:[{path:'images/visual.png',source:join(harness,'fixtures/transferred-minimum.png')}]});
});


test('captured OA square and mobile height retain physical dimensions across responsive aliases', {timeout:360000}, async()=> {
 await rendered(join(repo,'crates/devup-mcp-devup-ui/tests/fixtures/oa-media-size-aliases.json'),async(browser,url)=> {
  for(const width of [360,1440]) {
   const page=await browser.newPage({viewport:{width,height:900}});await page.goto(url);
   const media=page.locator(`[data-figma-node-ids~="${width}:${width===360?'73:9401':'73:9310'}"]`);
   const box=await media.boundingBox();
   assert.equal(box.width,width===360?320:560);
   assert.equal(box.height,width===360?240:560);
   const sizes=await media.evaluate(e=>({height:getComputedStyle(e).height,width:getComputedStyle(e).width}));
   assert.equal(sizes.height,width===360?'240px':'560px');
   await page.close();
  }
 },{responsive:true,reset:true,assets:[{path:'images/media.png',source:join(harness,'fixtures/transferred-minimum.png')},{path:'images/media-1.png',source:join(harness,'fixtures/transferred-minimum.png')}]});
});


test('captured Home mixed excluded borders preserve HUG rows and original content insets', {timeout:360000}, async()=> {
 const fixture=JSON.parse(await readFile(join(repo,'crates/devup-mcp-devup-ui/tests/fixtures/home-news-mixed-strokes.json'),'utf8'));
 for(const [root,width,cases] of [['desktop',1440,[['51:5838',77,24,23]]],['mobile',360,[['51:6703',68,20,19],['51:6704',66,20,19]]]]) {
  const snapshot=structuredClone(fixture);snapshot.roots=[root];
  await rendered(snapshot,async(browser,url)=> {
   const page=await browser.newPage({viewport:{width,height:900}});await page.goto(url);
   for(const [id,height,top,bottom] of cases) {
    const row=page.locator(`[data-figma-node-id="${id}"]`);await row.waitFor();
    const box=await row.boundingBox();const css=await row.evaluate(e=>{const c=getComputedStyle(e);return {top:c.paddingTop,bottom:c.paddingBottom,border:c.borderBottomWidth,height:c.height};});
    assert.ok(Math.abs(box.height-height)<0.1,JSON.stringify({root,id,box,css,height}));
    assert.equal(css.top,`${top}px`);assert.equal(css.bottom,`${bottom}px`);assert.equal(css.border,'1px');
   }
   await page.close();
  },{reset:true});
 }
});

test('reconciled title and summary render once and preserve parent layout through resize', {timeout:360000}, async()=> {
 await rendered(join(repo,'crates/devup-mcp-devup-ui/tests/fixtures/responsive-title-summary-wrappers.json'),async(browser,url)=> {
  const page=await browser.newPage({viewport:{width:360,height:900}});await page.goto(url);
  for(const width of [360,1440,360,1440]) {
   await page.setViewportSize({width,height:900});
   const eyebrow=page.getByText('PATENTS',{exact:true});
   const title=page.getByText('특허 · 지식재산권',{exact:true});
   const summary=page.getByText('전체 3건 / 등록 2건 / 출원 1건',{exact:true});
   const table=page.getByText('특허 목록',{exact:true});
   for(const element of [eyebrow,title,summary,table]) {await element.waitFor();assert.equal(await element.count(),1);}
   const bounds=await Promise.all([eyebrow,title,summary,table].map(element=>element.boundingBox()));
   const [captionBox,titleBox,summaryBox,tableBox]=bounds;
   const row=await title.evaluate(element=> {
    const titleGroup=element.parentElement,row=titleGroup.parentElement,root=row.parentElement;
    return {display:getComputedStyle(row).display,rootGap:getComputedStyle(root).rowGap,
     titleGap:getComputedStyle(titleGroup).rowGap,align:getComputedStyle(row).alignItems,
     justify:getComputedStyle(row).justifyContent};
   });
   assert.equal(row.rootGap,width===360?'24px':'32px');
   assert.equal(row.titleGap,width===360?'8px':'12px');
   assert.ok(Math.abs(titleBox.y-captionBox.y-captionBox.height-(width===360?8:12))<0.5,JSON.stringify({width,bounds,row}));
   if(width===360) {
    assert.equal(row.display,'contents');
    assert.ok(Math.abs(summaryBox.y-titleBox.y-titleBox.height-24)<0.5,JSON.stringify({width,bounds,row}));
    assert.ok(Math.abs(tableBox.y-summaryBox.y-summaryBox.height-24)<0.5,JSON.stringify({width,bounds,row}));
    assert.equal(summaryBox.x,titleBox.x);
   } else {
    assert.equal(row.display,'flex');assert.equal(row.align,'center');assert.equal(row.justify,'space-between');
    assert.ok(summaryBox.x>=titleBox.x+600,JSON.stringify({width,bounds,row}));
    assert.ok(Math.abs(summaryBox.y+summaryBox.height/2-(captionBox.y+(titleBox.y+titleBox.height-captionBox.y)/2))<0.5,JSON.stringify({width,bounds,row}));
    assert.ok(Math.abs(tableBox.y-titleBox.y-titleBox.height-32)<0.5,JSON.stringify({width,bounds,row}));
   }
  }
  await page.close();
 },{responsive:true,reset:true});
});
