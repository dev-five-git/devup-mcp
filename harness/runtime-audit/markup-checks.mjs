// Semantic and accessibility probes that do not depend on Figma attributes.
// Findings are measured DOM facts. Target-size exceptions are reported as
// review items rather than declaring every small inline link a WCAG failure.
export function collectMarkup() {
  const visible = (element) => {
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0 && (!element.checkVisibility
      || element.checkVisibility({ opacityProperty: true, visibilityProperty: true }));
  };
  const all = (selector) => [...document.querySelectorAll(selector)].filter((element) => visible(element)
    && !element.closest('[aria-hidden="true"],[inert]'));
  const describe = (element) => ({ tag: element.tagName.toLowerCase(), name: (element.getAttribute('aria-label')
    || element.textContent || element.getAttribute('alt') || '').replace(/\s+/g, ' ').trim().slice(0, 120) });
  const headings = all('h1,h2,h3,h4,h5,h6,[role="heading"]')
    .filter((element) => !element.closest('[aria-hidden="true"]'))
    .map((element) => ({ ...describe(element), level: Number(element.getAttribute('aria-level') || element.tagName.slice(1)) }));
  const missingAlt = all('img').filter((element) => !element.hasAttribute('alt')
    && !element.matches('[role="presentation"],[role="none"]')).map(describe);
  const smallTargets = all('a[href],button,input:not([type="hidden"]),select,textarea,[role="button"]')
    .filter((element) => !element.matches(':disabled,[aria-disabled="true"]'))
    .flatMap((element) => {
      const rect = element.getBoundingClientRect();
      return rect.width < 24 || rect.height < 24
        ? [{ ...describe(element), width: rect.width, height: rect.height, inline: getComputedStyle(element).display === 'inline' }]
        : [];
    });
  const footerViewport = all('footer:not(main footer):not(article footer):not(section footer),[role="contentinfo"]').map(element => ({
    bottom: element.getBoundingClientRect().bottom + window.scrollY,
    viewportHeight: window.innerHeight,
    documentHeight: Math.max(document.documentElement.scrollHeight, document.body?.scrollHeight ?? 0),
  }));
  return { bodyPresent: !!document.body, lang: document.documentElement.lang,
    landmarks: Object.fromEntries([
      ['header', 'header:not(main header):not(article header):not(section header),[role="banner"]'],
      ['nav', 'nav,[role="navigation"]'], ['main', 'main,[role="main"]'],
      ['footer', 'footer:not(main footer):not(article footer):not(section footer),[role="contentinfo"]'],
    ].map(([name, selector]) => [name, all(selector).length])), headings, missingAlt, smallTargets, footerViewport };
}

export function evaluateMarkup(observed) {
  if (!observed.bodyPresent) return { status: 'inconclusive', reason: 'document body is unavailable', measured: observed };
  const problems = [];
  if (!observed.lang?.trim()) problems.push('html language is missing');
  for (const name of ['header', 'nav', 'footer']) {
    if (!observed.landmarks[name]) problems.push(`${name} landmark is missing`);
  }
  if (observed.landmarks.main !== 1) problems.push(`expected one main landmark, found ${observed.landmarks.main}`);
  const h1 = observed.headings.filter((heading) => heading.level === 1).length;
  if (h1 !== 1) problems.push(`expected one h1, found ${h1}`);
  const headingSkips = observed.headings.flatMap((heading, index, headings) => index > 0
    && heading.level > headings[index - 1].level + 1 ? [heading] : []);
  if (headingSkips.length) problems.push(`${headingSkips.length} heading level skips`);
  if (observed.missingAlt.length) problems.push(`${observed.missingAlt.length} images have no alt attribute`);
  if (observed.footerViewport?.some(footer => footer.documentHeight <= footer.viewportHeight + 1 && footer.bottom < footer.viewportHeight - 1)) {
    problems.push('short-page footer ends above the viewport bottom; grow the shared main/page shell');
  }
  return { status: problems.length ? 'fail' : 'pass',
    reason: problems.length ? problems.join('; ') : 'semantic landmarks, language, headings and image alternatives present',
    measured: { ...observed, headingSkips, targetSizeReview: observed.smallTargets.length
      ? 'Targets below 24 CSS px need review of inline, spacing, equivalent-control or essential-size exceptions; size alone is not a conformance verdict.' : null } };
}
