// referencePng over the bridge. The direct path asks Figma's get_screenshot,
// which draws the node alone - no layer beside, above or below it - over the
// fills of the frames and sections it sits in and the page background, across
// its render bounds: its effects and overflowing children included, cut by
// clipping ancestors. A plugin cannot ask for that picture, so it draws the
// closest one it can and never refuses, always over the same bounds
// (useAbsoluteBounds: false), in sRGB, at its own size with the longer edge
// held to 8192px as the collector holds get_screenshot's maxDimension:
// - while the ancestors make one plain colour under the node (solid fills
//   covering it, on opaque unblended layers with no stroke, rounded corner,
//   inner effect or mask reaching it), the node alone (contentsOnly: true),
//   naming that colour for devup-mcp to flatten the export onto: the same
//   picture;
// - otherwise the area as the canvas shows it (contentsOnly: false), naming
//   the page background, the one thing that export leaves out. Other layers
//   reaching into the area are in that picture and not in get_screenshot's,
//   so `note` names them.
const nodeId = "__DEVUP_NODE_ID__";
const node = await figma.getNodeByIdAsync(nodeId);
if (!node || typeof node.exportAsync !== "function") {
  throw new Error(`DEVUP_NODE_NOT_FOUND ${nodeId}`);
}
const area = node.absoluteRenderBounds || node.absoluteBoundingBox;
const overlaps = (box) => !!box
  && box.x < area.x + area.width && area.x < box.x + box.width
  && box.y < area.y + area.height && area.y < box.y + box.height;
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
const label = (layer) => `${layer.type} ${JSON.stringify(layer.name)}`;

let mixed = null;
const mix = (why) => {
  mixed = mixed || why;
};
const colour = [255, 255, 255];
const paintOver = (paints, owner) => {
  for (const paint of shown(paints)) {
    if (paint.type !== "SOLID") mix(`${owner} is painted with ${paint.type}`);
    else if (paint.blendMode && paint.blendMode !== "NORMAL") mix(`${owner} blends its fill ${paint.blendMode}`);
    else {
      const alpha = paint.opacity === undefined ? 1 : paint.opacity;
      ["r", "g", "b"].forEach((channel, index) => {
        colour[index] = alpha * paint.color[channel] * 255 + (1 - alpha) * colour[index];
      });
    }
  }
};

const ancestors = [];
let page = node.parent;
while (page && page.type !== "PAGE") {
  ancestors.unshift(page);
  page = page.parent;
}
paintOver(page && page.backgrounds, "the page");
const pageColour = colour.slice();
for (const ancestor of ancestors) {
  const owner = label(ancestor);
  if (typeof ancestor.opacity === "number" && ancestor.opacity < 1) mix(`${owner} is translucent`);
  if (ancestor.blendMode && !["PASS_THROUGH", "NORMAL"].includes(ancestor.blendMode)) mix(`${owner} blends ${ancestor.blendMode}`);
  if (shown(ancestor.effects).some((effect) => effect.type !== "DROP_SHADOW")) mix(`${owner} has an inner effect`);
  const fills = shown(ancestor.fills);
  const stroke = shown(ancestor.strokes).length === 0 ? 0 : largest(ancestor.strokeWeight, ancestor.strokeTopWeight,
    ancestor.strokeRightWeight, ancestor.strokeBottomWeight, ancestor.strokeLeftWeight);
  const radius = largest(ancestor.cornerRadius, ancestor.topLeftRadius, ancestor.topRightRadius,
    ancestor.bottomLeftRadius, ancestor.bottomRightRadius);
  const box = ancestor.absoluteBoundingBox;
  if ((fills.length > 0 || stroke > 0) && !covers(box, stroke)) mix(`${owner} does not lie under it in one colour`);
  if (radius > 0 && (fills.length > 0 || ancestor.clipsContent) && reachesCorner(box, radius)) {
    mix(`${owner}'s rounded corner reaches it`);
  }
  paintOver(fills, owner);
}
const reaching = [];
for (let level = node; level.parent; level = level.parent) {
  const siblings = level.parent.children;
  if (siblings.slice(0, siblings.indexOf(level)).some((other) => other.isMask && other.visible !== false)) {
    mix("a mask clips it");
  }
  for (const other of siblings) {
    if (other !== level && !other.isMask && overlaps(other.absoluteRenderBounds)) reaching.push(label(other));
  }
}

const bytes = await node.exportAsync({
  format: "PNG",
  constraint: Math.max(area.width, area.height) > 8192
    ? { type: area.width >= area.height ? "WIDTH" : "HEIGHT", value: 8192 }
    : { type: "SCALE", value: 1 },
  contentsOnly: mixed === null,
  useAbsoluteBounds: false,
  colorProfile: "SRGB",
});
if (bytes.length === 0 || bytes.length > 8 * 1024 * 1024) {
  throw new Error(`DEVUP_REFERENCE_PNG_TOO_LARGE ${bytes.length} bytes, over the 8388608 one answer carries`);
}
const answer = {
  kind: "devupReferencePng",
  nodeId,
  mimeType: "image/png",
  byteLength: bytes.length,
  data: figma.base64Encode(bytes),
  backdrop: (mixed === null ? colour : pageColour).map(Math.round),
};
if (mixed !== null && reaching.length > 0) {
  const named = reaching.slice(0, 5).join(", ") + (reaching.length > 5 ? ` and ${reaching.length - 5} more` : "");
  answer.note = `Drawn as the canvas shows this area, because what lies under the node is not one colour (${mixed}). `
    + `${named} reach${reaching.length === 1 ? "es" : ""} into it and may show here; get_screenshot leaves other layers out.`;
}
return answer;
