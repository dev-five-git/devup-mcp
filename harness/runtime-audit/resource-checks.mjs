// Checks for the failure a fresh headless profile hides: a page whose images
// never arrived, whose buttons kept the browser's native face, or whose
// stylesheet differs from the one served a moment later. Observers run inside
// the page; evaluators are pure so each rule is testable without a browser.

const UA_GREY = new Set(['rgb(239, 239, 239)']);
const URL_PATTERN = /url\(\s*(?:"([^"]*)"|'([^']*)'|([^)]*?))\s*\)/g;

function round(value) {
  return Math.round(value * 100) / 100;
}

/** Refuse requests outside the plan's origins and remember what was refused. */
export async function guardOrigins(context, allowed, blocked) {
  await context.route('**/*', (route) => {
    const url = route.request().url();
    const protocol = url.slice(0, url.indexOf(':'));
    if (['data', 'blob', 'about'].includes(protocol) || allowed.has(new URL(url).origin)) {
      return route.continue();
    }
    blocked.push(url);
    return route.abort('blockedbyclient');
  });
}

/** Every rendered img and CSS background url in scope, loaded and measured. */
export function observeImages(scope) {
  return scope.evaluate(async (root) => {
    const rendered = (element) => !element.checkVisibility || element.checkVisibility({ visibilityProperty: true });
    const absolute = (source) => {
      try { return new URL(source, document.baseURI).href; } catch { return source; }
    };
    const label = (element) => element.id ? `#${element.id}` : `${element.tagName.toLowerCase()}${element.className && typeof element.className === 'string' ? `.${element.className.trim().split(/\s+/)[0]}` : ''}`;
    const images = [];
    const pending = [];
    let hidden = 0;
    for (const element of [root, ...root.querySelectorAll('*')]) {
      const shown = rendered(element);
      const rect = element.getBoundingClientRect();
      const box = { width: Math.round(rect.width * 100) / 100, height: Math.round(rect.height * 100) / 100 };
      if (element.tagName === 'IMG') {
        if (!shown) hidden += 1;
        else {
          const item = { kind: 'img', element: label(element), source: element.currentSrc || element.src || '', loaded: false, ...box };
          images.push(item);
          element.loading = 'eager';
          pending.push(new Promise((resolve) => {
            const timer = setTimeout(resolve, 5000);
            const done = () => { item.loaded = element.complete && element.naturalWidth > 0; clearTimeout(timer); resolve(); };
            element.decode().then(done, done);
          }));
        }
      }
      for (const pseudo of [null, '::before', '::after']) {
        const style = getComputedStyle(element, pseudo);
        if (pseudo && (style.content === 'none' || style.content === 'normal')) continue;
        for (const match of style.backgroundImage.matchAll(/url\(\s*(?:"([^"]*)"|'([^']*)'|([^)]*?))\s*\)/g)) {
          if (!shown) { hidden += 1; continue; }
          const source = absolute(match[1] ?? match[2] ?? match[3]);
          const item = { kind: `background${pseudo ?? ''}`, element: label(element), source, loaded: false, ...box };
          images.push(item);
          pending.push(new Promise((resolve) => {
            const picture = new Image();
            const timer = setTimeout(resolve, 5000);
            const done = () => { item.loaded = picture.naturalWidth > 0; clearTimeout(timer); resolve(); };
            picture.onload = done;
            picture.onerror = done;
            picture.src = source;
            if (picture.complete) done();
          }));
        }
      }
    }
    await Promise.all(pending);
    return { images, hidden };
  });
}

/**
 * @param {object} observed  { images: [{ kind, element, source, loaded, width, height }], hidden }
 * @param {object} requests  { responses: [{ url, status }], failed: [{ url, failure }], scoped }
 *   image requests made while the screen was open. When scoped, only requests
 *   for an image in scope count.
 */
export function evaluateImagesLoaded(observed, requests = {}) {
  const images = observed.images ?? [];
  const unpainted = images.flatMap((item) => {
    const why = [];
    if (!item.loaded) why.push('did not load');
    if (!(item.width > 0 && item.height > 0)) why.push('has no painted box');
    return why.length ? [{ ...item, problem: why.join(' and ') }] : [];
  });
  const sources = new Set(images.map((item) => item.source));
  const inScope = (item) => !requests.scoped || sources.has(item.url);
  const seen = new Set();
  const requestFailures = [
    ...(requests.responses ?? []).filter((item) => item.status >= 400).map((item) => ({ url: item.url, status: item.status })),
    ...(requests.failed ?? []).map((item) => ({ url: item.url, status: null, failure: item.failure })),
  ].filter(inScope).filter((item) => {
    const key = `${item.url} ${item.status}`;
    return seen.has(key) ? false : (seen.add(key), true);
  });
  const problems = [];
  if (unpainted.length) {
    problems.push(`${unpainted.length} images ${unpainted.slice(0, 5).map((item) => `${item.source || '(no src)'} on ${item.element} ${item.problem}`).join(', ')}`);
  }
  if (requestFailures.length) {
    problems.push(`${requestFailures.length} image requests failed: ${requestFailures.slice(0, 5).map((item) => `${item.status ?? item.failure} ${item.url}`).join(', ')}`);
  }
  return {
    status: problems.length ? 'fail' : 'pass',
    reason: problems.length ? problems.join('; ') : `${images.length} images and backgrounds loaded and painted`,
    measured: { listed: images.length, hidden: observed.hidden ?? 0, unpainted: unpainted.slice(0, 20), requestFailures: requestFailures.slice(0, 20) },
  };
}

/** Buttons and their computed appearance, against what the browser paints by default. */
export function observeNativeAppearance(scope) {
  return scope.evaluate((root) => {
    const isButton = (element) => element.tagName === 'BUTTON'
      || element.getAttribute('role') === 'button'
      || (element.tagName === 'INPUT' && ['button', 'submit', 'reset'].includes(element.type));
    const frame = document.createElement('iframe');
    frame.style.cssText = 'position:absolute;width:0;height:0;border:0;visibility:hidden';
    document.body.appendChild(frame);
    const defaults = new Map();
    const defaultsFor = (element) => {
      const key = `${element.tagName}:${element.type ?? ''}`;
      if (!defaults.has(key)) {
        const doc = frame.contentDocument;
        const probe = doc.createElement(element.tagName.toLowerCase());
        if (element.tagName === 'INPUT') probe.type = element.type;
        doc.body.appendChild(probe);
        const style = frame.contentWindow.getComputedStyle(probe);
        defaults.set(key, { background: style.backgroundColor, borderStyle: style.borderTopStyle, borderWidth: style.borderTopWidth, borderColor: style.borderTopColor });
      }
      return defaults.get(key);
    };
    const transparent = (color) => color === 'transparent' || /^rgba\(.*,\s*0\)$/.test(color);
    const controls = [];
    try {
      for (const element of [root, ...root.querySelectorAll('button, input, [role="button"]')]) {
        if (!isButton(element)) continue;
        if (element.checkVisibility && !element.checkVisibility({ visibilityProperty: true })) continue;
        const rect = element.getBoundingClientRect();
        if (!rect.width || !rect.height) continue;
        const style = getComputedStyle(element);
        const base = defaultsFor(element);
        const authorBackground = (!transparent(style.backgroundColor) && style.backgroundColor !== base.background)
          || (style.backgroundImage !== 'none');
        const authorBorder = style.borderTopStyle !== 'none' && parseFloat(style.borderTopWidth) > 0
          && (style.borderTopStyle !== base.borderStyle || style.borderTopWidth !== base.borderWidth || style.borderTopColor !== base.borderColor);
        controls.push({
          tag: element.tagName.toLowerCase(),
          type: element.type ?? null,
          name: (element.getAttribute('aria-label') || element.value || element.textContent || '').trim().slice(0, 40),
          appearance: style.appearance || style.webkitAppearance,
          backgroundColor: style.backgroundColor,
          borderStyle: style.borderTopStyle,
          borderWidth: style.borderTopWidth,
          authorBackground,
          authorBorder,
        });
      }
    } finally {
      frame.remove();
    }
    return { controls };
  });
}

/**
 * A button keeps the browser's native face unless the author set both a
 * background and a border; and a computed background of the UA default grey
 * is the native face leaking through whatever appearance says.
 */
export function evaluateNativeAppearance(observed) {
  const controls = observed.controls ?? [];
  const flagged = controls.flatMap((control) => {
    const why = [];
    if (control.appearance && control.appearance !== 'none' && !(control.authorBackground && control.authorBorder)) {
      why.push(`appearance ${control.appearance} without an author-set background and border`);
    }
    if (UA_GREY.has(control.backgroundColor)) why.push(`UA default grey background ${control.backgroundColor}`);
    return why.length ? [{ ...control, problems: why }] : [];
  });
  return {
    status: flagged.length ? 'fail' : 'pass',
    reason: flagged.length
      ? `${flagged.length} buttons show native browser chrome: ${flagged.slice(0, 5).map((item) => `<${item.tag}> "${item.name}" (${item.problems.join('; ')})`).join(', ')}`
      : `${controls.length} buttons are drawn by the author`,
    measured: { listed: controls.length, native: flagged.slice(0, 20) },
  };
}

/** Stylesheets (href + rule hash), image urls and the first section's height. */
export function snapshotLoad(page) {
  return page.evaluate(() => {
    const hash = (text) => {
      let h1 = 0xdeadbeef;
      let h2 = 0x41c6ce57;
      for (let index = 0; index < text.length; index += 1) {
        const code = text.charCodeAt(index);
        h1 = Math.imul(h1 ^ code, 2654435761);
        h2 = Math.imul(h2 ^ code, 1597334677);
      }
      h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507) ^ Math.imul(h2 ^ (h2 >>> 13), 3266489909);
      h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507) ^ Math.imul(h1 ^ (h1 >>> 13), 3266489909);
      return (4294967296 * (2097151 & h2) + (h1 >>> 0)).toString(16);
    };
    const stylesheets = [...document.styleSheets].map((sheet, index) => {
      let text = null;
      try { text = [...sheet.cssRules].map((rule) => rule.cssText).join('\n'); } catch { text = null; }
      return { href: sheet.href ?? `inline#${index}`, hash: text === null ? 'unreadable' : hash(text) };
    }).sort((a, b) => a.href.localeCompare(b.href));
    const images = new Set();
    for (const image of document.images) if (image.currentSrc || image.src) images.add(image.currentSrc || image.src);
    for (const element of document.querySelectorAll('*')) {
      for (const pseudo of [null, '::before', '::after']) {
        for (const match of getComputedStyle(element, pseudo).backgroundImage.matchAll(/url\(\s*(?:"([^"]*)"|'([^']*)'|([^)]*?))\s*\)/g)) {
          images.add(match[1] ?? match[2] ?? match[3]);
        }
      }
    }
    const section = document.querySelector('section') || document.querySelector('main > *') || document.body.firstElementChild;
    return {
      stylesheets,
      images: [...images].sort(),
      firstSection: section
        ? { name: section.id ? `#${section.id}` : section.tagName.toLowerCase(), height: Math.round(section.getBoundingClientRect().height * 100) / 100 }
        : null,
    };
  });
}

/** What changed between two loads of the same screen; empty when nothing did. */
export function diffLoads(first, second, tolerance = 0.5) {
  const diff = [];
  const sheets = (load) => new Map(load.stylesheets.map((sheet) => [sheet.href, sheet.hash]));
  const before = sheets(first);
  const after = sheets(second);
  for (const [href, hash] of before) {
    if (!after.has(href)) diff.push(`stylesheet ${href} missing from the fresh load`);
    else if (after.get(href) !== hash) diff.push(`stylesheet ${href} content changed (${hash} -> ${after.get(href)})`);
  }
  for (const href of after.keys()) if (!before.has(href)) diff.push(`stylesheet ${href} only in the fresh load`);
  const images = new Set(first.images);
  const fresh = new Set(second.images);
  for (const url of images) if (!fresh.has(url)) diff.push(`image ${url} missing from the fresh load`);
  for (const url of fresh) if (!images.has(url)) diff.push(`image ${url} only in the fresh load`);
  const [a, b] = [first.firstSection, second.firstSection];
  if ((a === null) !== (b === null)) diff.push('first section present in only one load');
  else if (a && Math.abs(a.height - b.height) > tolerance) diff.push(`first section ${a.name} height ${round(a.height)}px -> ${round(b.height)}px`);
  return diff;
}

/**
 * @param {object} observed  { responses: [{ url, status }] subresources of the first load,
 *   first, second: snapshotLoad results, skip: why no reload was made, reloadError }
 */
export function evaluateStaleResources(observed) {
  const errors = (observed.responses ?? []).filter((item) => item.status >= 400);
  const problems = [];
  if (errors.length) problems.push(`${errors.length} resources answered with an error: ${errors.slice(0, 5).map((item) => `${item.status} ${item.url}`).join(', ')}`);
  const diff = observed.first && observed.second ? diffLoads(observed.first, observed.second) : [];
  if (diff.length) problems.push(`the page differs from a fresh, cache-free load: ${diff.slice(0, 8).join('; ')}`);
  const measured = { responses: (observed.responses ?? []).length, errors: errors.slice(0, 20), diff: diff.slice(0, 40) };
  if (problems.length) return { status: 'fail', reason: problems.join('; '), measured };
  if (!observed.second) {
    return { status: 'inconclusive', reason: observed.skip ?? (observed.reloadError ? `the fresh load failed: ${observed.reloadError}` : 'no fresh load to compare'), measured };
  }
  return { status: 'pass', reason: 'stylesheets, images and first section height match a fresh, cache-free load', measured };
}

/** Load a url in a new context with the HTTP cache disabled and snapshot it. */
export async function freshLoad({ browser, contextOptions, install, url, timeout, ready }) {
  const context = await browser.newContext(contextOptions);
  try {
    await install(context);
    const page = await context.newPage();
    const session = await context.newCDPSession(page);
    await session.send('Network.enable');
    await session.send('Network.setCacheDisabled', { cacheDisabled: true });
    await page.goto(url, { waitUntil: 'load', timeout });
    await ready(page);
    return await snapshotLoad(page);
  } finally {
    await context.close();
  }
}
