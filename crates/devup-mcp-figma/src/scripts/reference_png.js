// referencePng over the bridge. The direct path asks Figma's get_screenshot,
// which answers with the node's area as the canvas shows it: a section's fill
// under the node included, the page background wherever nothing is drawn.
// A plugin cannot ask for that, so it exports the same area - the node and
// every layer overlapping it (contentsOnly: false) - at the node's own size
// with the longer edge held to 8192px (get_screenshot's maxDimension as the
// collector sets it), in sRGB (a browser capture's colour space), over the
// node's own box rather than the bounds its effects or overflow reach, which
// is the box generated code renders. It names the page background, and
// devup-mcp flattens the export onto it.
const nodeId = "__DEVUP_NODE_ID__";
const node = await figma.getNodeByIdAsync(nodeId);
if (!node || typeof node.exportAsync !== "function") {
  throw new Error(`DEVUP_NODE_NOT_FOUND ${nodeId}`);
}
let page = node;
while (page && page.type !== "PAGE") page = page.parent;
const backdrop = [255, 255, 255];
for (const paint of (page && page.backgrounds) || []) {
  if (!paint || paint.visible === false || paint.type !== "SOLID") continue;
  const alpha = paint.opacity === undefined ? 1 : paint.opacity;
  ["r", "g", "b"].forEach((channel, index) => {
    backdrop[index] = alpha * paint.color[channel] * 255 + (1 - alpha) * backdrop[index];
  });
}
const box = node.absoluteBoundingBox || node;
const bytes = await node.exportAsync({
  format: "PNG",
  constraint: Math.max(box.width, box.height) > 8192
    ? { type: box.width >= box.height ? "WIDTH" : "HEIGHT", value: 8192 }
    : { type: "SCALE", value: 1 },
  useAbsoluteBounds: true,
  contentsOnly: false,
  colorProfile: "SRGB",
});
if (bytes.length === 0 || bytes.length > 8 * 1024 * 1024) {
  throw new Error(`DEVUP_REFERENCE_PNG_TOO_LARGE ${bytes.length} bytes, over the 8388608 one answer carries`);
}
return {
  kind: "devupReferencePng",
  nodeId,
  mimeType: "image/png",
  byteLength: bytes.length,
  data: figma.base64Encode(bytes),
  backdrop: backdrop.map(Math.round),
};
