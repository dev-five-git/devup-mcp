import assert from 'node:assert/strict';
import test from 'node:test';
import {chromium} from 'playwright';
import {observeClippedContent} from '../clipping-checks.mjs';

test('animated descendants never hide unrelated vertical or horizontal clipping', async () => {
  const browser=await chromium.launch();
  try {
    const page=await browser.newPage();
    await page.setContent(`<style>@keyframes pulse{to{opacity:.8}} .animated{animation:pulse 1s infinite} #vertical{height:20px;overflow:hidden} #horizontal{width:100px;overflow:hidden;white-space:nowrap}</style>
      <section id="vertical"><span class="animated">10</span><p>Hidden content</p><p>More hidden content</p></section>
      <section id="horizontal"><span class="animated">10</span> Much wider visible business text that cannot fit</section>`);
    const result=await observeClippedContent(page);
    assert.ok(result.clipped.some(item=>item.name==='vertical' && item.vertical));
    assert.ok(result.clipped.some(item=>item.name==='horizontal' && item.horizontal));
  } finally { await browser.close(); }
});

test('only an explicitly named proven marquee is exempt, sibling clipping remains', async () => {
  const browser=await chromium.launch();
  try {
    const page=await browser.newPage();
    await page.setContent(`<style>@keyframes slide{to{transform:translateX(-50%)}} #marquee{width:100px;overflow:hidden} #track{width:max-content;white-space:nowrap;animation:slide 10s linear infinite} #broken{height:10px;overflow:hidden}</style>
      <div id="marquee"><div id="track">Regenerating joints restoring lives Regenerating joints restoring lives</div></div><div id="broken"><p>Required content</p><p>Cut off</p></div>`);
    assert.ok((await observeClippedContent(page)).clipped.some(item=>item.name==='marquee'));
    const result=await observeClippedContent(page,['#marquee']);
    assert.deepEqual(result.invalidExemptions,[]);
    assert.ok(!result.clipped.some(item=>item.name==='marquee'));
    assert.ok(result.clipped.some(item=>item.name==='broken'));
    for(const selector of ['body','#absent','#broken','[']) {
      assert.equal((await observeClippedContent(page,[selector])).invalidExemptions.length,1);
    }
    await page.addStyleTag({content:'@keyframes scaleOnly{to{transform:scaleX(.8)}} #track{animation-name:scaleOnly}'});
    const scaled=await observeClippedContent(page,['#marquee']);
    assert.equal(scaled.invalidExemptions.length,1,'an infinite scale animation is not horizontal translation');
    assert.ok(scaled.clipped.some(item=>item.name==='marquee'));
  } finally { await browser.close(); }
});
