import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { chromium } from 'playwright';
import { runRegressionCheck } from '../regression-checks.mjs';
import { runPlan } from '../run.mjs';

const title = 'A hand-thrown bowl with a brushed glaze.';
const contracts = {
  header: { kind: 'sticky-header', header: 'nav', hero: '#hero', logo: '#logo', links: 'nav a', scrolledBackground: 'rgb(246, 245, 243)' },
  inquiry: {
    kind: 'inquiry', open: '#open', detail: 'dialog[open]', description: 'dialog p',
    descriptionText: title, inquire: '#inquire', form: 'form',
    choice: { control: 'label[for=order]', selected: '#order', value: 'Commission' },
    fields: [{ selector: '#name', value: 'Test visitor' }, { selector: '#message', value: 'About the bowl' }],
    error: '#error', submit: '#send', recipient: 'fixture@example.test',
  },
  image: { kind: 'image-handoff', open: '#open', images: 'dialog img', next: '#next', nextSrc: '/second.svg' },
  font: { kind: 'font-stability', fonts: ['/wide.ttf'], targets: ['#label', '#menu'] },
};

function header(mode) {
  return `<style>
  body{margin:0}nav{box-sizing:border-box;position:fixed;inset:0 0 auto;height:56px;display:flex;justify-content:space-between;
    padding:16px;background:transparent; ${mode === 'snap' ? '' : 'transition:background-color 220ms ease-out'}}
  @media (prefers-reduced-motion: reduce){nav{transition:none}}
  nav.scrolled{background:#f6f5f3;box-shadow:inset 0 -1px #dcdbd6}
  nav a{text-decoration:none}
  #hero{height:100vh}main{height:150vh}
  ${mode === 'logo' ? 'nav:not(.scrolled) #logo{visibility:hidden}' : ''}
  ${mode === 'line' ? 'nav{border-bottom:1px solid black}' : ''}
  ${mode === 'underline' ? 'nav a:hover{text-decoration:underline}' : ''}
  </style><nav><a id="logo" href="/">Home</a><a href="#content">Work</a></nav><section id="hero"></section><main id="content"></main>
  <script>
  const nav=document.querySelector('nav'), hero=document.querySelector('#hero');
  addEventListener('scroll',()=>nav.classList.toggle('scrolled',hero.getBoundingClientRect().bottom<=nav.offsetHeight));
  document.querySelector('#logo').onclick=event=>{event.preventDefault();${mode === 'home' ? '' : "scrollTo(0,0)"}};
  </script>`;
}

function inquiry(mode) {
  return `<style>
  label{display:inline-block;padding:12px} ${mode === 'pressed' ? '' : 'label:active{opacity:.55}'}
  @keyframes show-error{from{opacity:.1;transform:translateY(-6px)}to{opacity:1}}
  #error:not([hidden]){${mode === 'error-motion' ? '' : 'animation:show-error 400ms ease-out'}}
  </style><button id="open">Open artwork</button><dialog><p>${mode === 'description' ? 'Wrong description' : title}</p>
  ${mode === 'cta' ? '' : '<button id="inquire">Ask about this work</button>'}</dialog>
  <form hidden><label for="order">Commission</label><input type="radio" name="choice" id="order" value="Commission">
  <input id="name"><textarea id="message"></textarea><button id="send">Send</button><p id="error" hidden>Required</p></form>
  <script>
  const dialog=document.querySelector('dialog'), form=document.querySelector('form'), error=document.querySelector('#error');
  document.querySelector('#open').onclick=()=>dialog.showModal();
  const inquire=document.querySelector('#inquire');
  if(inquire)inquire.onclick=()=>{dialog.close();form.hidden=false};
  ${mode === 'select' ? "document.querySelector('label').onclick=event=>event.preventDefault()" : ''}
  form.onsubmit=event=>{
    event.preventDefault();
    const fields=[document.querySelector('#name').value,document.querySelector('#message').value];
    if(fields.some(value=>!value)){error.hidden=false;return}
    const type=document.querySelector('#order').checked?'Commission':'Other';
    location.href='mailto:fixture@example.test?body='+encodeURIComponent([type,...fields].join(' '));
  };
  </script>`;
}

function image(mode) {
  return `<style>dialog img{touch-action:none}</style><button id="open">Artwork</button><dialog><img src="/first.svg" width="200" height="200"><button id="next">Next</button></dialog>
  <script>
  const dialog=document.querySelector('dialog'), current=dialog.querySelector('img');
  document.querySelector('#open').onclick=()=>dialog.showModal();
  const advance=()=>{
    ${mode === 'white' ? "current.src='/second.svg';" : "const candidate=new Image();candidate.src='/second.svg';candidate.decode().then(()=>current.src=candidate.src);"}
  };
  document.querySelector('#next').onclick=advance;
  let start;
  current.addEventListener('touchstart',event=>{start=event.touches[0].clientX});
  current.addEventListener('touchend',event=>{if(start-event.changedTouches[0].clientX>40)advance()});
  </script>`;
}

function font(mode) {
  return `${mode === 'preload' ? '' : '<link rel="preload" as="font" href="/wide.ttf" crossorigin>'}
  <style>
  @font-face{font-family:Wide;src:url('/wide.ttf');font-display:${mode === 'shift' ? 'swap' : 'optional'}}
  #row{display:flex;gap:8px}#label{font:30px Wide,monospace}
  </style><div id="row"><span id="label">MMMM</span><span id="menu">Contact</span></div>`;
}

test('UI regression contracts distinguish approved behavior from broken implementations', { timeout: 120000 }, async () => {
  const face = await readFile(new URL('../fixtures/regression-wide.ttf', import.meta.url));
  const fixtures = { header, inquiry, image, font };
  const server = createServer((request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (url.pathname === '/wide.ttf') {
      response.writeHead(200, { 'content-type': 'font/ttf' }).end(face);
    } else if (['/first.svg', '/second.svg'].includes(url.pathname)) {
      const color = url.pathname === '/first.svg' ? 'red' : 'blue';
      response.writeHead(200, { 'content-type': 'image/svg+xml' }).end(`<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"><rect width="200" height="200" fill="${color}"/></svg>`);
    } else {
      const name = url.searchParams.get('check');
      const render = fixtures[name];
      response.writeHead(render ? 200 : 404, { 'content-type': 'text/html' }).end(render ? '<!doctype html>' + render(url.searchParams.get('mode')) : 'missing fixture');
    }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch();
  try {
    async function check(name, mode, width) {
      const swipe = name === 'image' && width === 360;
      const page = await browser.newPage({ viewport: { width, height: 740 }, hasTouch: swipe, isMobile: swipe });
      const contract = swipe ? { ...contracts[name], next: undefined, swipe: 'left' } : contracts[name];
      page.setDefaultTimeout(2000);
      try {
        return await runRegressionCheck(page, contract, `http://127.0.0.1:${server.address().port}/?check=${name}&mode=${mode}`);
      } finally {
        await page.close();
      }
    }
    for (const width of [360, 1440]) {
      for (const name of Object.keys(contracts)) {
        const result = await check(name, 'good', width);
        assert.equal(result.status, 'pass', JSON.stringify({ name, width, result }));
      }
    }
    for (const [name, mode, reason] of [
      ['header', 'logo', /Logo/], ['header', 'home', /return to the top/],
      ['header', 'line', /no divider/], ['header', 'snap', /interpolate/],
      ['header', 'underline', /underlines/], ['inquiry', 'description', /description differs/],
      ['inquiry', 'cta', /#inquire/], ['inquiry', 'select', /was not selected/],
      ['inquiry', 'pressed', /pressed state/], ['inquiry', 'error-motion', /did not animate/],
      ['image', 'white', /lacked a painted artwork image/], ['font', 'preload', /not preloaded/],
      ['font', 'shift', /layout shifted/],
    ]) {
      const result = await check(name, mode, 360);
      assert.equal(result.status, 'fail', JSON.stringify({ name, mode, result }));
      assert.match(result.reason, reason, JSON.stringify({ name, mode, result }));
    }
    const dir = await mkdtemp(join(tmpdir(), 'devup-regression-plan-'));
    try {
      const baseUrl = 'http://127.0.0.1:' + server.address().port;
      const plan = join(dir, 'plan.json');
      await writeFile(plan, JSON.stringify({
        schemaVersion: 1, kind: 'devup-runtime-audit-plan', planId: 'sticky-regression',
        plan: {
          baseUrl, allowedOrigins: [baseUrl],
          viewports: [{ width: 360, height: 740 }],
          screens: ['good', 'line'].map(mode => ({
            id: mode, routePath: '/?check=header&mode=' + mode,
            checks: [{ kind: 'regression', regression: contracts.header }],
          })),
        },
      }));
      const report = await runPlan({ plan, out: join(dir, 'out'), maxMinutes: 1 });
      const [good, line] = report.runs[0].screens.map(screen => screen.checks[0]);
      assert.equal(good.status, 'pass', JSON.stringify(good));
      assert.equal(line.status, 'fail', JSON.stringify(line));
      assert.match(line.reason, /no divider/);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  } finally {
    await browser.close();
    await new Promise(resolve => server.close(resolve));
  }
});
