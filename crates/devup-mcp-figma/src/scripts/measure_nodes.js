// A fixed read of explicit IDs. Never walk children or acquire resources.
const ids = "__DEVUP_ROOT_IDS__";
if (!Array.isArray(ids) || ids.length < 1 || ids.length > 10 ||
    ids.some(id => typeof id !== 'string' || id.length > 128 || !/^(?:[0-9]+:[0-9]+|I[0-9]+:[0-9]+(?:;[0-9]+:[0-9]+)+)$/.test(id)) ||
    new Set(ids).size !== ids.length) throw new Error('DEVUP_INVALID_INPUT: nodeIds must be 1..10 distinct Figma IDs');
const number = (node, key) => {
  try { const value = node[key]; return typeof value === 'number' && Number.isFinite(value) ? value : null; }
  catch { return null; }
};
const label = (node, key) => {
  try { const value = node[key]; return typeof value === 'string' && value.length <= 32 && /^[A-Z_]+$/.test(value) ? value : null; }
  catch { return null; }
};
const nodes = [];
for (const nodeId of ids) {
  try {
    const node = await figma.getNodeByIdAsync(nodeId);
    if (!node || node.type === 'DOCUMENT' || node.type === 'PAGE') {
      nodes.push({nodeId, status: 'unavailable', reason: node ? 'not-scene-node' : 'not-found'});
      continue;
    }
    const layout = {
      width: number(node, 'width'), height: number(node, 'height'),
      padding: {top: number(node, 'paddingTop'), right: number(node, 'paddingRight'), bottom: number(node, 'paddingBottom'), left: number(node, 'paddingLeft')},
      gap: number(node, 'itemSpacing'), layoutMode: label(node, 'layoutMode'),
      align: {primary: label(node, 'primaryAxisAlignItems'), counter: label(node, 'counterAxisAlignItems')}
    };
    const known = layout.width !== null && layout.height !== null;
    nodes.push({nodeId, status: known ? 'measured' : 'partial', ...layout});
  } catch {
    nodes.push({nodeId, status: 'unavailable', reason: 'read-failed'});
  }
}
return {kind: 'devup-measure', schemaVersion: 1, coordinateSpace: 'figma-logical', nodes,
  partial: nodes.some(node => node.status !== 'measured'),
  evidenceLimit: 'Null means unavailable or not applicable; no screenshot dimensions, descendants, theme, assets or code were read.'};
