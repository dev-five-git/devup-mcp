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
// Figma answers a read of data it has not loaded (an instance sublayer whose
// main component lives elsewhere, in a file out of sight) only after 10 s.
// Reading the IDs one after another made two such IDs outlast the caller's
// time limit, so they are read together and each failure keeps its cause:
// the unreachable sentence sits in `error`, where the bridge recognises it
// and re-reads through the direct path, which does not depend on the view.
const read = async nodeId => {
  try {
    const node = await figma.getNodeByIdAsync(nodeId);
    if (!node || node.type === 'DOCUMENT' || node.type === 'PAGE') {
      return {nodeId, status: 'unavailable', reason: node ? 'not-scene-node' : 'not-found'};
    }
    const layout = {
      width: number(node, 'width'), height: number(node, 'height'),
      padding: {top: number(node, 'paddingTop'), right: number(node, 'paddingRight'), bottom: number(node, 'paddingBottom'), left: number(node, 'paddingLeft')},
      gap: number(node, 'itemSpacing'), layoutMode: label(node, 'layoutMode'),
      align: {primary: label(node, 'primaryAxisAlignItems'), counter: label(node, 'counterAxisAlignItems')}
    };
    const known = layout.width !== null && layout.height !== null;
    return {nodeId, status: known ? 'measured' : 'partial', ...layout};
  } catch (failure) {
    const message = String((failure && failure.message) || failure).slice(0, 240);
    return message.includes('Unable to establish connection to Figma')
      ? {nodeId, status: 'unavailable', reason: 'figma-unreachable', error: message}
      : {nodeId, status: 'unavailable', reason: 'read-failed'};
  }
};
const nodes = await Promise.all(ids.map(read));
return {kind: 'devup-measure', schemaVersion: 1, coordinateSpace: 'figma-logical', nodes,
  partial: nodes.some(node => node.status !== 'measured'),
  evidenceLimit: 'Null means unavailable or not applicable; no screenshot dimensions, descendants, theme, assets or code were read.'};
