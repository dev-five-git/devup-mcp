import assert from 'node:assert/strict';
import test from 'node:test';
import {createServer} from 'node:http';
import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {runPlan} from '../run.mjs';
import {sourceDigest} from '../source-digest.mjs';

test('browser evidence hashes actual sources before and after and rejects changed or stale plans', {timeout:30000}, async()=>{
  const root=await mkdtemp(join(tmpdir(),'devup-browser-source-'));
  const source=join(root,'page.tsx');let mutate=false,requests=0;
  await writeFile(source,'export function Page(){return <main>Initial</main>}');
  const server=createServer(async(_request,response)=>{
    requests++;
    if(mutate){mutate=false;await writeFile(source,'export function Page(){return <main>Changed</main>}');}
    response.setHeader('content-type','text/html');response.end('<main><h1>Measured page</h1></main>');
  });
  await new Promise(done=>server.listen(0,'127.0.0.1',done));
  try{
    const baseUrl=`http://127.0.0.1:${server.address().port}`;
    const digest=(await sourceDigest(root)).sourceDigest;
    const plan={schemaVersion:1,kind:'devup-runtime-audit-plan',planId:'source-binding',plan:{baseUrl,allowedOrigins:[baseUrl],projectRoot:root,sourceDigest:digest,viewports:[{width:360,height:300}],screens:[{id:'home',routePath:'/',checks:[{kind:'page-health'}]}]}};
    await mkdir(join(root,'.devup'),{recursive:true});const path=join(root,'.devup/plan.json');await writeFile(path,JSON.stringify(plan));
    const stable=await runPlan({plan:path,out:join(root,'.devup/stable'),maxMinutes:.2});
    assert.equal(stable.status,'completed');assert.equal(stable.sourceDigest,digest);
    assert.equal(stable.sourceVerification.beforeDigest,digest);assert.equal(stable.sourceVerification.afterDigest,digest);
    assert.equal(stable.sourceVerification.unchanged,true);assert.equal(stable.sourceVerification.planMatched,true);
    mutate=true;
    const changed=await runPlan({plan:path,out:join(root,'.devup/changed'),maxMinutes:.2});
    assert.equal(changed.status,'aborted');assert.equal(changed.sourceVerification.unchanged,false);assert.notEqual(changed.sourceVerification.afterDigest,digest);
    const beforeRequests=requests;
    const stale=await runPlan({plan:path,out:join(root,'.devup/stale'),maxMinutes:.2});
    assert.equal(stale.status,'aborted');assert.equal(stale.sourceVerification.planMatched,false);assert.notEqual(stale.sourceDigest,digest);assert.equal(requests,beforeRequests,'stale sources are rejected before browser navigation');
    delete plan.plan.projectRoot;await writeFile(path,JSON.stringify(plan));
    const missing=await runPlan({plan:path,out:join(root,'.devup/missing'),maxMinutes:.2});
    assert.equal(missing.status,'aborted');assert.equal(missing.sourceDigest,null);assert.match(missing.error,/projectRoot/);
  }finally{await new Promise(done=>server.close(done));await rm(root,{recursive:true,force:true});}
});
