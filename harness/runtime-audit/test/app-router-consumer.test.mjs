import assert from 'node:assert/strict';
import test from 'node:test';
import {cp,mkdir,mkdtemp,writeFile,readFile,rm} from 'node:fs/promises';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createServer} from 'node:net';
import {join,resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {chromium} from 'playwright';
import {preparedBinaries,renderFixture} from '../prepared-binaries.mjs';
import {mcpClient} from '../test-support/mcp-client.mjs';
import {runMotionCheck} from '../motion-checks.mjs';

const harness=resolve(dirname(fileURLToPath(import.meta.url)),'..'), execute=promisify(execFile);
const binaries=await preparedBinaries();
async function port(){const server=createServer();await new Promise(done=>server.listen(0,'127.0.0.1',done));const value=server.address().port;await new Promise(done=>server.close(done));return value;}
async function ready(url,child,logs){
  for(let attempt=0;attempt<100;attempt++){
    if(child.exitCode!==null)throw new Error(`Consumer exited ${child.exitCode}: ${logs()}`);
    try{const result=await fetch(url);if(result.ok)return;}catch{}
    await new Promise(done=>setTimeout(done,200));
  }throw new Error(`Consumer did not start: ${logs()}`);
}

test('installed MCP guidance and clean generated TSX run in a production App Router consumer',{timeout:180_000},async()=>{
  await mkdir(join(harness,'out'),{recursive:true});const dir=await mkdtemp(join(harness,'out','app-router-'));
  let server,browser,mcp,logs='';
  try{
    await cp(join(harness,'fixtures/app-router'),dir,{recursive:true});
    // Reuse the existing harness asset for this synthetic consumer's tab icon.
    await mkdir(join(dir,'public'),{recursive:true});await cp(join(harness,'fixtures/rule.svg'),join(dir,'public/favicon.svg'));
    for(const file of ['MotionController.tsx','motion-runtime.ts']) await cp(join(harness,'../../crates/devup-mcp/src/server/behavior_contracts/templates',file),join(dir,'src/components',file));
    const snapshot={fileKey:'consumer-contract',version:'1',roots:['mobile','desktop'],nodes:{},diagnostics:[]};
    for(const [id,width,padding]of[['mobile',360,40],['desktop',1440,80]]){
      const title=`title-${id}`,body=`body-${id}`;
      snapshot.nodes[id]={id,type:'FRAME',fields:{name:'Consumer content',width,height:300,layoutMode:'VERTICAL',layoutSizingHorizontal:'FILL',layoutSizingVertical:'HUG',paddingTop:16,paddingBottom:padding,paddingLeft:16,paddingRight:16,itemSpacing:16,boundVariables:{paddingBottom:{type:'VARIABLE_ALIAS',id:'space'}},childrenIds:[title,body]}};
      snapshot.nodes[title]={id:title,type:'TEXT',fields:{name:'Page title',parentId:id,characters:'Consumer research',fontSize:28,layoutSizingHorizontal:'FILL',textAutoResize:'HEIGHT',textTruncation:'DISABLED'}};
      snapshot.nodes[body]={id:body,type:'TEXT',fields:{name:'Description',parentId:id,characters:'한글 단어는 자연스럽게 줄바꿈되어야 합니다.',fontSize:16,layoutSizingHorizontal:'FILL',textAutoResize:'HEIGHT',textTruncation:'DISABLED'}};
    }
    const capture=join(dir,'capture.json');await writeFile(capture,JSON.stringify(snapshot));
    const output=join(dir,'src/components/Generated.tsx');
    const generated=await renderFixture(capture,output,{responsive:true,clean:true,theme:join(dir,'theme-bindings.json')});
    assert.equal(generated.status,0,generated.stderr);const source=await readFile(output,'utf8');
    assert.match(source,/\$containerY/);assert.doesNotMatch(source,/data-(figma|source)-node|\/\/\s*@devup-(?:interaction|override|overlay)/);
    mcp=await mcpClient(binaries.mcp,dir);
    const guide=await mcp.request('resources/read',{uri:'devup://guide/usage'});
    assert.match(guide.contents.map(item=>item.text??'').join('\n'),/Behavior and data contracts/);
    await mcp.call('devup_skills',{action:'install',projectRoot:dir,names:['devfive-frontend','devup-implementation-plan','devup']});
    assert.match(await readFile(join(dir,'.agents/skills/devfive-frontend/references/delivery-quality.md'),'utf8'),/sidecar/);
    const validation=await mcp.call('devup_ui_validate',{projectRoot:dir,paths:['src/components/Generated.tsx'],strict:true});
    assert.equal(validation.ok,true,JSON.stringify(validation));
    await mcp.close();mcp=null;
    const cli=join(harness,'node_modules/vinext/dist/cli.js');
    await execute(process.execPath,[cli,'build'],{cwd:dir,windowsHide:true,timeout:120_000,maxBuffer:16*1024*1024});
    await execute(process.execPath,[join(harness,'node_modules/typescript/bin/tsc'),'--noEmit'],{cwd:dir,windowsHide:true,timeout:30_000,maxBuffer:8*1024*1024});
    const listen=await port(),base=`http://127.0.0.1:${listen}`;
    server=spawn(process.execPath,[cli,'start','--port',String(listen),'--hostname','127.0.0.1'],{cwd:dir,windowsHide:true,stdio:['ignore','pipe','pipe']});
    server.stdout.on('data',value=>{logs=(logs+value).slice(-6000);});server.stderr.on('data',value=>{logs=(logs+value).slice(-6000);});
    await ready(base,server,()=>logs);
    const html=await fetch(base).then(response=>response.text());assert.match(html,/Consumer research/);assert.doesNotMatch(html,/data-figma-node|<!--\s*@devup-/);
    browser=await chromium.launch();
    const page=await browser.newPage({viewport:{width:360,height:1100}}),errors=[];
    page.on('pageerror',error=>errors.push(error.message));await page.goto(base);
    await page.getByRole('heading',{name:'Consumer research'}).waitFor();
    for(const width of [359,360,479,480,767,768,991,992,1279,1280,1440]){
      await page.setViewportSize({width,height:1100});
      const values=await page.getByRole('heading',{name:'Consumer research'}).evaluate(element=>{
        const parent=element.parentElement,body=[...parent.children].find(node=>node!==element);
        return {padding:getComputedStyle(parent).paddingBottom,wordBreak:getComputedStyle(body).wordBreak,overflow:document.documentElement.scrollWidth>innerWidth,footer:document.querySelector('footer').getBoundingClientRect().bottom};
      });
      assert.equal(values.padding,width>=1280?'80px':'40px',JSON.stringify({width,values}));assert.equal(values.wordBreak,'keep-all');assert.equal(values.overflow,false);assert.ok(values.footer>=1099);
    }
    await page.evaluate(()=>{window.__consumerTabs=document.getElementById('consumer-tabs');});
    const slide=await runMotionCheck(page,{kind:'tabs',candidateId:'sliding-tab-indicator:consumer',target:'#consumer-tabs',links:'a',indicator:'#consumer-tab-indicator',next:'#consumer-tabs a[href="/details"]',durationMs:400});
    assert.equal(slide.status,'pass',JSON.stringify(slide));await page.getByRole('heading',{name:'Detail route'}).waitFor();
    assert.equal(await page.evaluate(()=>window.__consumerTabs===document.getElementById('consumer-tabs')),true,'shared layout remains mounted across navigation');
    assert.equal(await page.getByRole('link',{name:'Details',exact:true}).getAttribute('aria-current'),'page');
    const back=await runMotionCheck(page,{kind:'tabs',candidateId:'sliding-tab-indicator:consumer',target:'#consumer-tabs',links:'a',indicator:'#consumer-tab-indicator',next:'#consumer-tabs a[href="/"]',durationMs:400});
    assert.equal(back.status,'pass',JSON.stringify(back));await page.getByRole('link',{name:'Summary',exact:true}).waitFor();
    assert.equal(await page.getByRole('link',{name:'Summary',exact:true}).getAttribute('aria-current'),'page');
    assert.deepEqual(errors,[]);
    const nojs=await browser.newContext({javaScriptEnabled:false,viewport:{width:360,height:1100}});const staticPage=await nojs.newPage();await staticPage.goto(base);assert.equal(await staticPage.getByRole('heading',{name:'Consumer research'}).isVisible(),true);await nojs.close();
  }finally{
    await mcp?.close();if(browser)await browser.close();
    if(server){server.kill();await new Promise(done=>{if(server.exitCode!==null)done();else server.once('exit',done);});}
    await rm(dir,{recursive:true,force:true});
  }
});
