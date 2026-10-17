// Operation-time read: independent of asynchronous selection event delivery.
const page = figma.currentPage;
const selected = page.selection;
const ref = node => ({id: node.id, name: String(node.name).slice(0, 256), type: node.type});
return {kind: 'devup-bridge-context', currentPage: ref(page),
  selection: selected.slice(0, 20).map(ref), selectionCount: selected.length,
  zoom: Number.isFinite(figma.viewport?.zoom) ? figma.viewport.zoom : null,
  observedAt: Date.now(), lastKnownSelection: globalThis.__devupLastSelection ?? null,
  lastKnownPage: globalThis.__devupLastSelectionPage ?? null};
