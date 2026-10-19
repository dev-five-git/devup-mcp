import assert from 'node:assert/strict';
import test from 'node:test';
import { resolveLinks } from '../run.mjs';

test('links resolve once per concrete URL and fail errors and unplanned targets', async () => {
  const calls = [];
  const request = { get: async (url) => {
    calls.push(url);
    return { status: () => url.endsWith('/missing') ? 404 : 200,
      url: () => url, text: async () => url.endsWith('/error') ? "This page couldn't load" : 'OK' };
  } };
  const anchors = ['/posts/one', '/posts/one', '/missing', '/error', '/invented'].map((href) => ({from:'http://local/', label:href, href, pathname:href, width:390}));
  const cache = new Map();
  const rows = await resolveLinks(request, anchors, {plannedTargets:['/', '/posts/[slug]', '/missing', '/error']}, cache);
  assert.equal(calls.length, 4);
  assert.deepEqual(rows.map((row) => row.status), ['pass','pass','fail','fail','fail']);
  assert.equal(rows[4].reason, 'unplanned-target');
  assert.equal(rows[0].planned, true);
  await resolveLinks(request, anchors, {plannedTargets:['/posts/[slug]']}, cache);
  assert.equal(calls.length, 4);
});

test('runner records anchors at each width and follows redirects to errors', async () => {
  const {createServer} = await import('node:http');
  const {mkdtemp, writeFile, rm} = await import('node:fs/promises');
  const {tmpdir} = await import('node:os');
  const {join} = await import('node:path');
  const {runPlan} = await import('../run.mjs');
  const counts = new Map();
  const server = createServer((req, res) => {
    counts.set(req.url, (counts.get(req.url) ?? 0) + 1);
    if (req.url === '/redirect') {res.writeHead(302, {location:'/missing'}); res.end(); return;}
    if (req.url === '/missing') res.statusCode = 404;
    res.setHeader('content-type', 'text/html');
    res.end(req.url === '/' ? '<a href="/posts/one" aria-label="First post">One</a><a href="/posts/one">Again</a><a href="/redirect">Bad</a><a href="/unknown">Unknown</a><a href="/existing">Existing</a><a href="/soft">Soft</a>' : req.url === '/soft' ? "This page couldn't load" : 'OK');
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const dir = await mkdtemp(join(tmpdir(), 'devup-links-'));
  try {
    const baseUrl = `http://127.0.0.1:${server.address().port}`;
    const path = join(dir, 'plan.json');
    await writeFile(path, JSON.stringify({schemaVersion:1, kind:'devup-runtime-audit-plan', planId:'links', plan:{baseUrl, allowedOrigins:[baseUrl], viewports:[390,1440].map((width) => ({width,height:900,deviceScaleFactor:1})), screens:[{id:'home',routePath:'/',checks:[{kind:'links-resolve', plannedTargets:['/', '/posts/[slug]', '/redirect', '/soft'], existingRoutes:['/existing']}]}]}}));
    const report = await runPlan({plan:path,out:join(dir,'out'),maxMinutes:1});
    assert.equal(report.status, 'completed');
    assert.equal(report.linkTable.length, 12);
    assert.equal(counts.get('/posts/one'), 1);
    assert.equal(counts.get('/redirect'), 1);
    assert.equal(report.summary.fail, 2);
    assert.match(report.linkTableMarkdown, /First post/);
    for (const run of report.runs) {
      const rows = run.screens[0].linkTable;
      assert.deepEqual(rows.map((row) => row.status), ['pass','pass','fail','fail','pass','fail']);
      assert.equal(rows[0].width, run.viewport.width);
      assert.equal(rows[0].from, baseUrl + '/');
    }
  } finally {await new Promise((resolve) => server.close(resolve)); await rm(dir,{recursive:true,force:true});}
});


test('a required catch-all needs a concrete segment but an optional one may be empty',async()=> {
  const request={get:async(url)=>({status:()=>200,url:()=>url,text:async()=> 'OK'})};
  const anchors=[{from:'http://local/',href:'/posts',label:'Posts',pathname:'/posts',width:390}];
  assert.equal((await resolveLinks(request,anchors,{plannedTargets:['/posts/[...slug]']}))[0].reason,'unplanned-target');
  assert.equal((await resolveLinks(request,anchors,{plannedTargets:['/posts/[[...slug]]']}))[0].status,'pass');
});
