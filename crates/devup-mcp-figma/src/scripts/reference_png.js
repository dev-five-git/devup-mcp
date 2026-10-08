// referencePng over the bridge. The direct path asks Figma's get_screenshot;
// a plugin cannot, so it exports the node as Figma draws it instead - at its
// own size with the longer edge held to 8192px (get_screenshot's maxDimension
// as the collector sets it), in sRGB (a browser capture's colour space), over
// the node's own box rather than the bounds its effects or overflow reach,
// which is the box generated code renders.
const nodeId = "__DEVUP_NODE_ID__";
const node = await figma.getNodeByIdAsync(nodeId);
if (!node || typeof node.exportAsync !== "function") {
  throw new Error(`DEVUP_NODE_NOT_FOUND ${nodeId}`);
}
const box = node.absoluteBoundingBox || node;
const bytes = await node.exportAsync({
  format: "PNG",
  constraint: Math.max(box.width, box.height) > 8192
    ? { type: box.width >= box.height ? "WIDTH" : "HEIGHT", value: 8192 }
    : { type: "SCALE", value: 1 },
  useAbsoluteBounds: true,
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
};
