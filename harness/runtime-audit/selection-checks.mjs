// A URL change or aria-current alone cannot prove the active paint followed it.
export async function assertSelection(page, input, locate, timeout) {
  const deadline = Date.now() + timeout;
  let measured = {};
  do {
    const group = locate(page, input.group), target = locate(page, input.target);
    if (await group.count() === 1 && await target.count() === 1 && await target.isVisible()) {
      const handle = await group.elementHandle();
      try {
        const inside = await target.evaluate((element, group) => group.contains(element), handle);
        const candidates = await group.locator(`[${input.attribute}]`).all();
        const selected = [];
        for (const candidate of candidates) {
          if (await candidate.isVisible()) {
            const value = await candidate.getAttribute(input.attribute);
            if (value && value !== 'false') selected.push(value);
          }
        }
        const attributes = await target.getAttribute(input.attribute);
        const paint = await target.evaluate((element, properties) => {
          const style = getComputedStyle(element);
          return Object.fromEntries(properties.map(property => [property, style.getPropertyValue(property) || style[property]]));
        }, Object.keys(input.styles));
        const content = !input.content || (await locate(page, input.content).count() === 1 && await locate(page, input.content).isVisible());
        measured = { inside, selectedCount: selected.length, attribute: attributes, paint, contentVisible: content };
        if (inside && selected.length === 1 && attributes === input.value && content
          && Object.entries(input.styles).every(([property, expected]) => paint[property] === expected)) return measured;
      } finally { await handle?.dispose(); }
    }
    if (Date.now() < deadline) await page.waitForTimeout(Math.min(50, deadline - Date.now()));
  } while (Date.now() < deadline);
  throw new Error(`selected control/content/paint are inconsistent: ${JSON.stringify(measured)}`);
}
