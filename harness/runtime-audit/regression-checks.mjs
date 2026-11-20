// These contracts run in a fresh context, without frozen animations or shared
// route state. The parent installs its usual origin guard before navigation.
const PAINT = ['backgroundColor', 'color', 'opacity', 'transform', 'boxShadow', 'outlineColor', 'outlineWidth', 'borderColor'];
const TRANSPARENT = 'rgba(0, 0, 0, 0)';
const HELP = new Map([
  ['sticky-header', 'Sticky header state, motion and home action'],
  ['inquiry', 'Detail and inquiry form flow'],
  ['image-handoff', 'Artwork image handoff'],
  ['font-stability', 'First-viewport font stability'],
]);

async function one(page, selector) {
  const locator = page.locator(selector);
  await locator.waitFor({ state: 'visible' }).catch(() => {
    throw new Error('Element did not become visible: ' + selector);
  });
  if (await locator.count() !== 1) throw new Error('Expected one visible element: ' + selector);
  return locator;
}

const paint = locator => locator.evaluate((element, fields) => {
  const style = getComputedStyle(element);
  return fields.map(field => style[field]);
}, PAINT);

async function pressed(page, locator) {
  const touch = await page.evaluate(() => navigator.maxTouchPoints > 0);
  if (touch) await locator.click({ trial: true });
  else await locator.hover();
  await page.waitForTimeout(150);
  const before = await paint(locator);
  const session = touch ? await page.context().newCDPSession(page) : null;
  try {
    if (session) {
      const box = await locator.boundingBox();
      await session.send('Input.dispatchTouchEvent', {
        type: 'touchStart', touchPoints: [{ x: Math.floor(box.x + box.width / 2), y: Math.floor(box.y + box.height / 2) }],
      });
    } else {
      await page.mouse.down();
    }
    let changed = false;
    let underlineFree = true;
    for (let sample = 0; sample < 10; sample++) {
      await page.evaluate(() => new Promise(requestAnimationFrame));
      const after = await paint(locator);
      changed ||= before.some((value, index) => value !== after[index]);
      underlineFree &&= await decoration(locator);
    }
    return { changed, decoration: underlineFree };
  } finally {
    if (session) {
      await session.send('Input.dispatchTouchEvent', { type: 'touchCancel', touchPoints: [] }).catch(() => {});
      await session.detach().catch(() => {});
    } else {
      await page.mouse.move(-1, -1);
      await page.mouse.up();
    }
  }
}

const decoration = locator => locator.evaluate(element => ![element, ...element.querySelectorAll('*')]
  .some(node => getComputedStyle(node).textDecorationLine.includes('underline')));

async function stickyHeader(page, config) {
  const { header, hero, logo, links, scrolledBackground } = config;
  const nav = await one(page, header);
  const atTop = await nav.evaluate(element => {
    const box = element.getBoundingClientRect();
    return { height: box.height, top: box.top };
  });
  const logoAtTop = await page.locator(logo).evaluate(el => {
    const box = el.getBoundingClientRect();
    return el.checkVisibility({ opacityProperty: true, visibilityProperty: true }) &&
      box.x >= 0 && box.y >= 0 && box.right <= innerWidth && box.bottom <= innerHeight;
  }).catch(() => false);
  const sample = async (down, reduced) => nav.evaluate(async (element, options) => {
    const { down, hero, reduced } = options;
    const next = down ? document.querySelector(hero).getBoundingClientRect().bottom + scrollY : 0;
    const top = element.getBoundingClientRect().top;
    const height = element.getBoundingClientRect().height;
    const read = () => {
      const paints = [null, '::before', '::after'].map(pseudo => getComputedStyle(element, pseudo))
        .filter((style, index) => index === 0 || !['none', 'normal'].includes(style.content));
      const root = paints[0];
      const divider = paints.some(style => {
        const shadow = style.boxShadow.replace(/rgba?\([^)]*\)/g, '');
        return (parseFloat(style.borderBottomWidth) > 0 && style.borderBottomStyle !== 'none' &&
            style.borderBottomColor !== 'rgba(0, 0, 0, 0)') ||
          /(?:^|,)\s*0px -[\d.]+px [^,]*\binset\b/.test(shadow) ||
          (style.position === 'absolute' && style.bottom === '0px' && parseFloat(style.height) <= 2 &&
            parseFloat(style.height) > 0 && style.backgroundColor !== 'rgba(0, 0, 0, 0)');
      });
      return { background: root.backgroundColor, divider };
    };
    const start = read();
    scrollTo({ top: next, behavior: 'instant' });
    const colors = new Set();
    const startTime = performance.now();
    let stable = true;
    let transitioned = false;
    while (performance.now() - startTime < (reduced ? 150 : 500)) {
      await new Promise(requestAnimationFrame);
      colors.add(read().background);
      const box = element.getBoundingClientRect();
      stable &&= Math.abs(box.top - top) < 1 && Math.abs(box.height - height) < 1;
      transitioned ||= element.getAnimations().some(animation => animation.transitionProperty === 'background-color');
    }
    const end = read();
    return {
      start, end, stable, transitioned,
      intermediate: [...colors].filter(value => value !== start.background && value !== end.background).length,
      y: scrollY,
    };
  }, { hero, down, reduced });

  const down = await sample(true, false);
  const up = await sample(false, false);
  const linkDecorations = async () => {
    const current = page.locator(links);
    const count = await current.count();
    if (count > 16) throw new Error('Sticky header contract accepts at most 16 links.');
    const states = [];
    for (let index = 0; index < count; index++) {
      const link = current.nth(index);
      if (!(await link.isVisible())) continue;
      await page.mouse.move(-1, -1);
      const resting = await decoration(link);
      await link.hover();
      await page.waitForTimeout(150);
      const hovered = await decoration(link);
      const active = await pressed(page, link);
      states.push({ resting, hovered, pressed: active.decoration });
    }
    return states;
  };
  const heroLinks = await linkDecorations();
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const reduced = await sample(true, true);
  const scrolledLinks = await linkDecorations();
  const linkStates = { hero: heroLinks, scrolled: scrolledLinks };
  const scrolledLogo = await one(page, logo);
  const homePath = new URL(page.url()).pathname;
  const homeHref = await scrolledLogo.evaluate(el => el instanceof HTMLAnchorElement && new URL(el.href).origin === location.origin &&
    new URL(el.href).pathname === location.pathname && !new URL(el.href).hash);
  if (homeHref) await scrolledLogo.click();
  const returnedHome = homeHref && await page.waitForFunction(path => scrollY < 5 && location.pathname === path && !location.hash, homePath, { timeout: 1500 })
    .then(() => true, () => false);
  const issues = [];
  if (!logoAtTop || !returnedHome) issues.push('Logo must be visible on the hero and return to the top from the scrolled state.');
  if (down.start.background !== TRANSPARENT || up.end.background !== TRANSPARENT || down.start.divider || up.end.divider) {
    issues.push('The hero state must be transparent and have no divider.');
  }
  if (down.end.background !== scrolledBackground || !down.end.divider) issues.push('Scrolled background or divider differs from the approved state.');
  if (down.y < 5 || !down.stable || !up.stable || atTop.top > 1) issues.push('The header must stay in place while scrolling.');
  if (!(down.intermediate > 0 && up.intermediate > 0 && down.transitioned && up.transitioned)) issues.push('Background must interpolate in both directions.');
  if (reduced.transitioned || reduced.intermediate !== 0 || reduced.end.background !== scrolledBackground) issues.push('Reduced motion must apply the scrolled endpoint immediately.');
  if (Object.values(linkStates).some(links => !links.length || links.some(state => !state.resting || !state.hovered || !state.pressed))) {
    issues.push('Navigation links show underlines in a tested state, or no links were measured.');
  }
  return { issues, measured: { logoAtTop, homeHref, returnedHome, down, up, reduced, linkStates } };
}

async function inquiry(page, config) {
  await (await one(page, config.open)).click();
  await one(page, config.detail);
  const description = await one(page, config.description);
  const descriptionMatches = (await description.innerText()).replace(/\s+/g, ' ').trim() === config.descriptionText.replace(/\s+/g, ' ').trim();
  await (await one(page, config.inquire)).click();
  await one(page, config.form);
  const submit = await one(page, config.submit);
  const session = await page.context().newCDPSession(page);
  const drafts = [];
  const record = event => { if (event.url.startsWith('mailto:')) drafts.push(new URL(event.url)); };
  await session.send('Page.enable');
  session.on('Page.frameRequestedNavigation', record);
  try {
    await submit.click();
    const shownError = await one(page, config.error);
    const validationMotion = await shownError.evaluate(el => el.getAnimations({ subtree: true }).length > 0);
    const rejectedEmpty = drafts.length === 0;
    const choice = await one(page, config.choice.control);
    const press = await pressed(page, choice);
    const selectedTarget = config.choice.selected ? page.locator(config.choice.selected) : null;
    const matchesSelection = () => selectedTarget.evaluate((element, value) =>
      element instanceof HTMLInputElement ? element.checked && element.value === value : element.textContent.trim() === value, config.choice.value);
    let selected;
    let initiallySelected;
    if (config.choice.selected) {
      initiallySelected = await selectedTarget.count() === 1 && await matchesSelection();
      await choice.click();
      if (config.choice.option) await (await one(page, config.choice.option)).click();
      selected = await matchesSelection();
    } else {
      initiallySelected = (await choice.inputValue()) === config.choice.value;
      await choice.selectOption(config.choice.value);
      selected = (await choice.inputValue()) === config.choice.value;
    }
    for (const field of config.fields) await (await one(page, field.selector)).fill(field.value);
    await submit.click();
    for (let attempt = 0; attempt < 12 && drafts.length === 0; attempt++) await page.waitForTimeout(50);
    const draft = drafts.at(-1);
    const body = draft?.searchParams.get('body') ?? '';
    const subject = draft?.searchParams.get('subject') ?? '';
    const carriesFields = config.fields.every(({value}) => body.includes(value)) &&
      (subject.includes(config.choice.value) || body.includes(config.choice.value));
    const recipientMatches = Boolean(draft) && decodeURIComponent(draft.pathname) === config.recipient;
    const measured = { descriptionMatches, rejectedEmpty, validationMotion, pressed: press.changed, selected: !initiallySelected && selected, carriesFields, recipientMatches };
    const reasons = {
      descriptionMatches: 'Artwork description differs from the approved copy.',
      rejectedEmpty: 'Empty submission opened an email draft.',
      validationMotion: 'Required-field feedback did not animate.',
      pressed: 'The inquiry type control has no measured pressed state.',
      selected: 'The expected inquiry type was not selected.',
      carriesFields: 'The draft is missing the selected inquiry type or field values.',
      recipientMatches: 'Submission did not open the expected mailto draft.',
    };
    const issues = Object.entries(measured).filter(([, pass]) => !pass).map(([key]) => reasons[key]);
    return { issues, measured };
  } finally {
    await session.detach().catch(() => {});
  }
}

function holdResources(page, urlList) {
  const urls = new Set(urlList);
  const started = new Set();
  const succeeded = new Set();
  let release;
  const held = new Promise(resolve => { release = resolve; });
  const route = page.route(url => urls.has(url.href), async request => {
    started.add(request.request().url());
    await held;
    await request.fallback();
  });
  page.on('response', response => {
    if (urls.has(response.url()) && response.ok()) succeeded.add(response.url());
  });
  return { route, started, succeeded, release };
}

const matchesReadyImage = ({ images, nextSrc }) => [...document.querySelectorAll(images)].some(img =>
  img instanceof HTMLImageElement && img.currentSrc === nextSrc && img.complete && img.naturalWidth > 0 &&
  img.checkVisibility({ opacityProperty: true, visibilityProperty: true }));

async function swipeArtwork(page, target, direction) {
  await target.scrollIntoViewIfNeeded();
  const box = await target.boundingBox();
  const x = Math.floor(box.x + box.width / 2);
  const y = Math.floor(box.y + box.height / 2);
  const end = x + (direction === 'left' ? -1 : 1) * Math.min(Math.floor(box.width / 3), 100);
  const session = await page.context().newCDPSession(page);
  try {
    await session.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y }] });
    await session.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: end, y }] });
    await session.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  } finally {
    await session.detach().catch(() => {});
  }
}

async function imageHandoff(page, config, url) {
  const nextSrc = new URL(config.nextSrc, url).href;
  const held = holdResources(page, [nextSrc]);
  await held.route;
  try {
    await page.addInitScript(expected => {
      const decode = HTMLImageElement.prototype.decode;
      let release;
      const ready = new Promise(resolve => { release = resolve; });
      const gate = { observed: false, release };
      window.__devupDecodeGate = gate;
      HTMLImageElement.prototype.decode = function () {
        const decoded = decode.call(this);
        if ((this.currentSrc || this.src) !== expected) return decoded;
        gate.observed = true;
        return Promise.all([decoded, ready]).then(() => undefined);
      };
    }, nextSrc);
    await page.goto(url, { waitUntil: 'domcontentloaded' });
    if (config.ready) await page.locator(config.ready).waitFor({ state: 'attached' });
    else await page.waitForLoadState('load', { timeout: 1000 }).catch(() => {});
    if (config.open) await (await one(page, config.open)).click();
    await page.waitForFunction(images => [...document.querySelectorAll(images)].some(img => img.complete && img.naturalWidth > 0 &&
      img.checkVisibility({ opacityProperty: true, visibilityProperty: true })), config.images)
      .catch(() => { throw new Error("The starting artwork was not visibly loaded."); });
    const start = await page.evaluate(images => [...document.querySelectorAll(images)]
      .find(img => img.complete && img.naturalWidth > 0 && img.checkVisibility({ opacityProperty: true, visibilityProperty: true })).currentSrc, config.images);
    if (start === nextSrc) throw new Error('The starting image already uses nextSrc.');
    await page.evaluate(images => {
      const audit = { samples: 0, blankFrames: 0, running: true };
      window.__devupImageAudit = audit;
      const sample = () => {
        if (!audit.running) return;
        audit.samples++;
        if (![...document.querySelectorAll(images)].some(img => {
          if (!(img instanceof HTMLImageElement && img.complete && img.naturalWidth > 0 &&
            img.checkVisibility({ opacityProperty: true, visibilityProperty: true }))) return false;
          const box = img.getBoundingClientRect();
          return box.bottom > 0 && box.top < innerHeight && box.right > 0 && box.left < innerWidth;
        })) audit.blankFrames++;
        requestAnimationFrame(sample);
      };
      requestAnimationFrame(sample);
    }, config.images);
    if (config.swipe) await swipeArtwork(page, await one(page, config.images), config.swipe);
    else await (await one(page, config.next)).click();
    await page.waitForTimeout(300);
    const heldRequest = held.started.has(nextSrc);
    held.release();
    await page.waitForFunction(() => window.__devupDecodeGate.observed, null, { timeout: 650 }).catch(() => {});
    const released = await page.evaluate(config => {
      const gate = window.__devupDecodeGate;
      const visibleBeforeDecode = [...document.querySelectorAll(config.images)].some(img =>
        img.currentSrc === config.nextSrc && img.checkVisibility({ opacityProperty: true, visibilityProperty: true }));
      gate.release();
      return { observedDecode: gate.observed, visibleBeforeDecode };
    }, { images: config.images, nextSrc });
    await page.waitForFunction(matchesReadyImage, { images: config.images, nextSrc })
      .catch(() => { throw new Error("The expected replacement artwork did not become visible."); });
    const decoded = await page.locator(config.images).evaluateAll(async (images, src) => {
      const img = images.find(image => image.currentSrc === src);
      return img?.decode().then(() => true, () => false) ?? false;
    }, nextSrc);
    await page.waitForTimeout(250);
    const { samples, blankFrames } = await page.evaluate(() => {
      window.__devupImageAudit.running = false;
      return window.__devupImageAudit;
    });
    const issues = [];
    if (!heldRequest) issues.push('The next-image response was not held during the handoff.');
    if (samples < 2 || blankFrames > 0) issues.push('At least one handoff frame lacked a painted artwork image, or frames were not observed.');
    if (!released.observedDecode || released.visibleBeforeDecode) issues.push('The replacement became visible without a measured decode wait.');
    if (!decoded || !held.succeeded.has(nextSrc)) issues.push('The expected replacement image did not load and decode.');
    return { issues, measured: { heldRequest, decoded, samples, blankFrames, ...released } };
  } finally {
    held.release();
  }
}

const positions = (page, selectors) => page.evaluate(selectors => selectors.map(selector => {
  const nodes = [...document.querySelectorAll(selector)];
  if (nodes.length !== 1) throw new Error('Font stability target needs exactly one element: ' + selector);
  const { x, y, width, height } = nodes[0].getBoundingClientRect();
  if (!width || !height || y >= innerHeight || y + height <= 0) throw new Error('Font stability target is outside the first viewport: ' + selector);
  return { x, y };
}), selectors);

async function fontStability(page, config, url) {
  const fonts = config.fonts.map(path => new URL(path, url).href);
  const held = holdResources(page, fonts);
  await held.route;
  try {
    await page.addInitScript(() => {
      window.__devupFontShifts = [];
      if (!PerformanceObserver.supportedEntryTypes.includes('layout-shift')) return;
      new PerformanceObserver(list => {
        for (const entry of list.getEntries()) {
          if (!entry.hadRecentInput) window.__devupFontShifts.push(entry.value);
        }
      }).observe({ type: 'layout-shift', buffered: true });
    });
    await page.goto(url, { waitUntil: 'domcontentloaded' });
    for (const selector of config.targets) await one(page, selector);
    await page.waitForFunction(() => performance.now() >= 1500 &&
      performance.getEntriesByName('first-contentful-paint').length > 0, null, { timeout: 5000 });
    const before = await positions(page, config.targets);
    const started = fonts.every(font => held.started.has(font));
    const preloaded = await page.evaluate(fonts => {
      const hrefs = [...document.querySelectorAll('link[rel="preload"][as="font"]')]
        .filter(link => !link.media || matchMedia(link.media).matches).map(link => link.href);
      return fonts.every(font => hrefs.includes(font));
    }, fonts);
    held.release();
    await page.waitForLoadState('load');
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(100);
    const after = await positions(page, config.targets);
    const maxMove = Math.max(...after.map((box, index) => Math.hypot(box.x - before[index].x, box.y - before[index].y)));
    const cls = await page.evaluate(() => PerformanceObserver.supportedEntryTypes.includes('layout-shift')
      ? window.__devupFontShifts.reduce((sum, value) => sum + value, 0) : null);
    const loaded = fonts.every(font => held.succeeded.has(font));
    const issues = [];
    if (!started || !preloaded || !loaded) issues.push('Expected first-viewport font was not preloaded and fetched.');
    if (maxMove > 0.5 || cls === null || cls > 0.001) issues.push('First-viewport layout shifted while fonts loaded, or CLS was not observed.');
    return { issues, measured: { preloaded, loaded, started, maxMove, cls } };
  } finally {
    held.release();
  }
}

export async function runRegressionCheck(page, config, url) {
  try {
    let result;
    if (config.kind === 'image-handoff') result = await imageHandoff(page, config, url);
    else if (config.kind === 'font-stability') result = await fontStability(page, config, url);
    else if (config.kind === 'sticky-header' || config.kind === 'inquiry') {
      await page.goto(url, { waitUntil: 'load' });
      await page.evaluate(() => document.fonts.ready);
      result = await (config.kind === 'sticky-header' ? stickyHeader : inquiry)(page, config);
    } else {
      return { status: 'inconclusive', reason: 'Unknown browser regression contract.', measured: {} };
    }
    return { status: result.issues.length ? 'fail' : 'pass', reason: result.issues.join(' ') || HELP.get(config.kind), measured: result.measured };
  } catch (error) {
    return { status: 'fail', reason: String(error.message ?? error).split('\n')[0], measured: {} };
  }
}
