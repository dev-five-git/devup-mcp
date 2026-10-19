import assert from 'node:assert/strict';
import test from 'node:test';
import {spawnSync} from 'node:child_process';
import {mkdir, mkdtemp, writeFile, readFile, rm} from 'node:fs/promises';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {build, preview} from 'vite';
import {DevupUI} from '@devup-ui/vite-plugin';
import {chromium} from 'playwright';

const harness = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repo = resolve(harness, '../..');
async function rendered(snapshot, verify) {
  await mkdir(join(harness,'out'),{recursive:true});
  const dir = await mkdtemp(join(harness,'out','layout-'));
  let server, browser;
  try {
    if (typeof snapshot !== 'string') {await writeFile(join(dir,'snapshot.json'),JSON.stringify(snapshot));snapshot=join(dir,'snapshot.json');}
    const generated = spawnSync('cargo',['run','--quiet','-p','devup-mcp-devup-ui','--example','render_fixture','--',snapshot,join(dir,'Generated.tsx')],{cwd:repo,encoding:'utf8',windowsHide:true,timeout:300000});
    assert.equal(generated.status,0,generated.stderr);
    await writeFile(join(dir,'devup.json'),JSON.stringify({theme:{}}));
    await writeFile(join(dir,'index.html'),'<html><body style="margin:0"><div id="root"></div><script type="module" src="/main.tsx"></script></body></html>');
    await writeFile(join(dir,'main.tsx'),`import {createRoot} from 'react-dom/client';import {RenderFixture} from './Generated';createRoot(document.getElementById('root')).render(<RenderFixture/>);`);
    const config={root:dir,configFile:false,logLevel:'error',plugins:[DevupUI()],build:{outDir:'dist'},preview:{host:'127.0.0.1',port:0}};
    await build(config); server=await preview(config); browser=await chromium.launch();
    await verify(browser,`http://127.0.0.1:${server.httpServer.address().port}`);
  } finally {
    if(browser) await browser.close();
    if(server) await new Promise((done)=>server.httpServer.close(done));
    await rm(dir,{recursive:true,force:true});
  }
}

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
    Object.assign(root.fields,{width:drawn,height:100,itemSpacing:20,inferredAutoLayout:{layoutMode:'HORIZONTAL',itemSpacing:20},absoluteBoundingBox:{x:0,y:0,width:drawn,height:100}});
    for(const id of root.fields.childrenIds) snapshot.nodes[id]={id,type:'FRAME',fields:{parentId:'root',width:itemWidth,height:100,layoutSizingHorizontal:'FIXED',layoutSizingVertical:'FIXED',maxWidth:null,maxHeight:null}};
    await rendered(snapshot,async(browser,url)=> {
      for(const width of [390,drawn]) {
        const page=await browser.newPage({viewport:{width,height:900}});
        await page.goto(url);
        await page.locator(`[data-figma-node-id="item${count-1}"]`).waitFor();
        const items=await Promise.all(root.fields.childrenIds.map((id)=>page.locator(`[data-figma-node-id="${id}"]`).boundingBox()));
        assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth),width);
        assert.ok(items.every((box)=>box.x+box.width<=width+0.5));
        if(width===drawn) for(const [i,box] of items.entries()) {
          assert.equal(box.x,i*(itemWidth+20)); assert.equal(box.y,0); assert.equal(box.width,itemWidth);
        }
        else assert.ok(items.at(-1).y>items[0].y);
        await page.close();
      }
    });
  }
});
