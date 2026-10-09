// referencePng over the bridge. The direct path asks Figma's get_screenshot,
// which draws the node alone - no layer beside, above or below it - over the
// fills of the frames and sections it sits in and the page background, across
// its render bounds: its effects and overflowing children included, cut by
// clipping ancestors. A plugin cannot ask for that picture. It can export the
// node alone over the same bounds (contentsOnly: true, useAbsoluteBounds:
// false) in sRGB, at its own size with the longer edge held to 8192px as the
// collector holds get_screenshot's maxDimension, and name the colour under it
// for devup-mcp to flatten the export onto. That is the same picture only
// when the ancestors make one colour: plain solid fills covering the node, on
// opaque unblended layers with no stroke, rounded corner, inner effect or mask
// reaching it. Otherwise this throws DEVUP_REFERENCE_PNG_NOT_REPRODUCIBLE and
// the read goes to get_screenshot instead of coming back as another picture.
const nodeId = "__DEVUP_NODE_ID__";
const node = await figma.getNodeByIdAsync(nodeId);
if (!node || typeof node.exportAsync !== "function") {
  throw new Error(`DEVUP_NODE_NOT_FOUND ${nodeId}`);
}
const area = node.absoluteRenderBounds || node.absoluteBoundingBox;
const refuse = (why) => {
  throw new Error(`DEVUP_REFERENCE_PNG_NOT_REPRODUCIBLE ${nodeId}: ${why}`);
};
const covers = (box, inset) => !!box
  && box.x + inset <= area.x && box.y + inset <= area.y
  && box.x + box.width - inset >= area.x + area.width
  && box.y + box.height - inset >= area.y + area.height;
const reachesCorner = (box, radius) => [
  [box.x, box.y], [box.x + box.width - radius, box.y],
  [box.x, box.y + box.height - radius], [box.x + box.width - radius, box.y + box.height - radius],
].some(([x, y]) => x < area.x + area.width && area.x < x + radius && y < area.y + area.height && area.y < y + radius);
const largest = (...values) => Math.max(0, ...values.filter((value) => typeof value === "number"));
const shown = (paints) => (Array.isArray(paints) ? paints : []).filter((paint) => paint && paint.visible !== false);

const backdrop = [255, 255, 255];
const paintOver = (paints, owner) => {
  for (const paint of shown(paints)) {
    if (paint.type !== "SOLID") refuse(`${owner} is painted with ${paint.type}`);
    if (paint.blendMode && paint.blendMode !== "NORMAL") refuse(`${owner} blends its fill ${paint.blendMode}`);
    const alpha = paint.opacity === undefined ? 1 : paint.opacity;
    ["r", "g", "b"].forEach((channel, index) => {
      backdrop[index] = alpha * paint.color[channel] * 255 + (1 - alpha) * backdrop[index];
    });
  }
};

const ancestors = [];
let page = node.parent;
while (page && page.type !== "PAGE") {
  ancestors.unshift(page);
  page = page.parent;
}
paintOver(page && page.backgrounds, "the page");
for (const ancestor of ancestors) {
  const owner = `${ancestor.type} ${JSON.stringify(ancestor.name)}`;
  if (typeof ancestor.opacity === "number" && ancestor.opacity < 1) refuse(`${owner} is translucent`);
  if (ancestor.blendMode && !["PASS_THROUGH", "NORMAL"].includes(ancestor.blendMode)) {
    refuse(`${owner} blends ${ancestor.blendMode}`);
  }
  if (shown(ancestor.effects).some((effect) => effect.type !== "DROP_SHADOW")) refuse(`${owner} has an inner effect`);
  const fills = shown(ancestor.fills);
  const stroke = shown(ancestor.strokes).length === 0 ? 0 : largest(ancestor.strokeWeight, ancestor.strokeTopWeight,
    ancestor.strokeRightWeight, ancestor.strokeBottomWeight, ancestor.strokeLeftWeight);
  const radius = largest(ancestor.cornerRadius, ancestor.topLeftRadius, ancestor.topRightRadius,
    ancestor.bottomLeftRadius, ancestor.bottomRightRadius);
  const box = ancestor.absoluteBoundingBox;
  if ((fills.length > 0 || stroke > 0) && !covers(box, stroke)) refuse(`${owner} does not lie under it in one colour`);
  if (radius > 0 && (fills.length > 0 || ancestor.clipsContent) && reachesCorner(box, radius)) {
    refuse(`${owner}'s rounded corner reaches it`);
  }
  paintOver(fills, owner);
}
for (let level = node; level.parent; level = level.parent) {
  const below = level.parent.children.slice(0, level.parent.children.indexOf(level));
  if (below.some((other) => other.isMask && other.visible !== false)) refuse("a mask clips it");
}

const bytes = await node.exportAsync({
  format: "PNG",
  constraint: Math.max(area.width, area.height) > 8192
    ? { type: area.width >= area.height ? "WIDTH" : "HEIGHT", value: 8192 }
    : { type: "SCALE", value: 1 },
  contentsOnly: true,
  useAbsoluteBounds: false,
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
