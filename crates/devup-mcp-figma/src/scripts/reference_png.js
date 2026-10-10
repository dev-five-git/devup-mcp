// referencePng over the bridge. The direct path asks Figma's get_screenshot,
// which draws the node in its ancestors' context - their paints, clipping,
// opacity, blend modes and blur - with every other layer left out, across the
// node's render bounds (measured on the devup Test file's cases). Exports are
// at the node's size with the longer edge held to 8192px, as the collector
// holds get_screenshot's maxDimension, in sRGB.
// - No other layer draws into those bounds: the canvas export (contentsOnly:
//   false) is that picture; devup-mcp flattens it onto the page background.
// - One does: a plugin cannot leave it out of an export, so this exports the
//   node alone and describes its ancestors - and sends the images their fills
//   use, and the masks that clip the chain - for devup-mcp to compose.
// - What devup-mcp cannot compose is named in `approximate`, over the canvas
//   export; the read then goes to get_screenshot when the direct path can.
// An answer too large for one bridge answer is held here and read in pieces
// (referencePngChunk). A held answer stays until it is read or two minutes
// pass; while held answers fill the budget this answers BUSY, and devup-mcp
// asks again, rather than drop one another request is still reading.
const nodeId = "__DEVUP_NODE_ID__";
const PIECE_THRESHOLD = 6 * 1024 * 1024;
const ANSWER_LIMIT = 90 * 1024 * 1024; // under the 96 MiB devup-mcp reads back
const HELD_BUDGET = 192 * 1024 * 1024;
const HELD_FOR = 2 * 60 * 1000;
if (!globalThis.__devupReferencePngAnswers) globalThis.__devupReferencePngAnswers = new Map();
const store = globalThis.__devupReferencePngAnswers;
let heldLength = 0;
for (const [id, entry] of store) {
  if (Date.now() - entry.at > HELD_FOR) store.delete(id);
  else heldLength += entry.text.length;
}
if (heldLength >= HELD_BUDGET) {
  throw new Error(`DEVUP_REFERENCE_PNG_BUSY ${heldLength} characters are held for other reads`);
}
const node = await figma.getNodeByIdAsync(nodeId);
if (!node || typeof node.exportAsync !== "function") {
  throw new Error(`DEVUP_NODE_NOT_FOUND ${nodeId}`);
}
const area = node.absoluteRenderBounds || node.absoluteBoundingBox;
const longest = Math.max(area.width, area.height);
const scale = longest > 8192 ? 8192 / longest : 1;
const settings = (contentsOnly, own) => ({
  format: "PNG",
  constraint: own && scale < 1
    ? { type: area.width >= area.height ? "WIDTH" : "HEIGHT", value: 8192 }
    : { type: "SCALE", value: scale },
  contentsOnly,
  useAbsoluteBounds: false,
  colorProfile: "SRGB",
});
const shown = (list) => (Array.isArray(list) ? list : []).filter((item) => item && item.visible !== false);
const overlaps = (a, b) => !!a && !!b
  && a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
const covers = (box, inset) => !!box
  && box.x + inset <= area.x && box.y + inset <= area.y
  && box.x + box.width - inset >= area.x + area.width && box.y + box.height - inset >= area.y + area.height;
const label = (layer) => `${layer.type} ${JSON.stringify(layer.name)}`;
const plain = (value) => JSON.parse(JSON.stringify(value === figma.mixed ? null : value));
const pngSize = (bytes) => [16, 20].map((at) => ((bytes[at] << 24) | (bytes[at + 1] << 16) | (bytes[at + 2] << 8) | bytes[at + 3]) >>> 0);
const normal = (layer) => (layer.opacity === undefined || layer.opacity === 1)
  && (!layer.blendMode || ["PASS_THROUGH", "NORMAL"].includes(layer.blendMode));

const chain = [];
let page = node.parent;
while (page && page.type !== "PAGE") {
  chain.unshift(page);
  page = page.parent;
}
const pageColour = [255, 255, 255];
for (const paint of shown(page && page.backgrounds)) {
  if (paint.type !== "SOLID") continue;
  const alpha = paint.opacity === undefined ? 1 : paint.opacity;
  ["r", "g", "b"].forEach((channel, index) => {
    pageColour[index] = alpha * paint.color[channel] * 255 + (1 - alpha) * pageColour[index];
  });
}

// What else draws into the area: a visible layer outside the node's chain with
// a paint, a stroke, an effect or text there, itself or below it.
let visited = 0;
const draws = (layer) => {
  if (layer.visible === false || layer.isMask || !overlaps(layer.absoluteRenderBounds, area)) return false;
  visited += 1;
  if (visited > 5000) return true;
  if (layer.type === "TEXT" || shown(layer.fills).length || shown(layer.strokes).length || shown(layer.effects).length) return true;
  return "children" in layer && layer.children.some(draws);
};
// An opaque plain paint over the whole area hides whatever lies under it.
const hidesUnder = (layer) => normal(layer) && covers(layer.absoluteBoundingBox, 0)
  && Math.abs(layer.absoluteTransform[0][1]) < 1e-9 && Math.abs(layer.absoluteTransform[1][0]) < 1e-9
  && Math.max(0, ...[layer.topLeftRadius, layer.topRightRadius, layer.bottomRightRadius, layer.bottomLeftRadius]
    .filter((radius) => typeof radius === "number")) === 0
  && shown(layer.fills).some((paint) => (paint.blendMode || "NORMAL") === "NORMAL"
    && (paint.opacity === undefined || paint.opacity === 1)
    && (paint.type === "SOLID" || (paint.type.startsWith("GRADIENT_") && paint.gradientStops.every((stop) => stop.color.a === 1))));
const members = [...chain, node];
const intruders = [];
const below = [];
const masks = new Map();
for (let depth = 0; depth < members.length; depth += 1) {
  const member = members[depth];
  const siblings = member.parent.children;
  const index = siblings.indexOf(member);
  const underneath = members.slice(depth, members.length - 1);
  const hidden = underneath.some((layer, at) => hidesUnder(layer) && underneath.slice(0, at).every(normal));
  siblings.forEach((other, at) => {
    if (other === member) return;
    if (other.isMask && other.visible !== false && at < index) masks.set(member, other);
    if (!draws(other) || (at < index && hidden)) return;
    intruders.push(label(other));
    if (at < index) below.push(other);
  });
}

const pageBackdrop = pageColour.map(Math.round);
const canvasAnswer = async (extra) => {
  const bytes = await node.exportAsync(settings(false, true));
  return { kind: "devupReferencePng", nodeId, mimeType: "image/png", byteLength: bytes.length, data: figma.base64Encode(bytes), backdrop: pageBackdrop, ...extra };
};

// The canvas picture stands in for what devup-mcp cannot compose, and says why.
const approximation = (reasons, reaching) => {
  const unique = [...new Set(reasons)];
  return {
    approximate: unique.join("; "),
    note: `Drawn as the canvas shows this area: devup-mcp cannot yet compose what get_screenshot draws here (${unique.slice(0, 3).join("; ")}). `
      + `Layers reaching into it may show here: ${reaching}; get_screenshot leaves other layers out.`,
  };
};

const held = (answer) => {
  // Escaped to ASCII, so an offset into it counts the same here (UTF-16 units)
  // and in devup-mcp (bytes): layer names in a note are often not ASCII.
  const text = JSON.stringify(answer)
    .replace(/[\u0080-\uffff]/g, (unit) => `\\u${unit.charCodeAt(0).toString(16).padStart(4, "0")}`);
  if (text.length > ANSWER_LIMIT) {
    if (answer.compose) {
      // What it takes to compose does not fit; the canvas picture alone may.
      const { compose, reaching, ...canvas } = answer;
      return held({ ...canvas, ...approximation([`the answer would be ${text.length} characters`], reaching) });
    }
    throw new Error(`DEVUP_REFERENCE_PNG_TOO_LARGE ${text.length} characters, over the ${ANSWER_LIMIT} one answer is read back up to`);
  }
  if (text.length <= PIECE_THRESHOLD) return answer;
  const id = `${nodeId}@${Date.now()}.${Math.random().toString(36).slice(2, 10)}`;
  store.set(id, { text, at: Date.now() });
  return { kind: "devupReferencePngChunked", nodeId, id, length: text.length };
};

if (intruders.length === 0) return held(await canvasAnswer({}));
// Inside a mask's own layers the node only shapes the mask: Figma draws none of
// it, and get_screenshot shows what lies under it.
const shapesMask = members.some((member) => member.isMask);

const approximate = [];
const images = new Set();
const radii = (layer) => [layer.topLeftRadius, layer.topRightRadius, layer.bottomRightRadius, layer.bottomLeftRadius]
  .map((radius) => (typeof radius === "number" ? radius : 0));
const strokeReaches = (layer) => {
  const weights = shown(layer.strokes).length === 0 ? [] : [layer.strokeWeight, layer.strokeTopWeight, layer.strokeRightWeight,
    layer.strokeBottomWeight, layer.strokeLeftWeight].filter((weight) => typeof weight === "number" && weight > 0);
  if (weights.length === 0) return false;
  const weight = Math.max(...weights);
  const inner = layer.strokeAlign === "OUTSIDE" ? 0 : layer.strokeAlign === "CENTER" ? weight / 2 : weight;
  const outer = layer.strokeAlign === "INSIDE" ? 0 : layer.strokeAlign === "CENTER" ? weight / 2 : weight;
  const box = layer.absoluteBoundingBox;
  const grown = box && { x: box.x - outer, y: box.y - outer, width: box.width + 2 * outer, height: box.height + 2 * outer };
  return overlaps(grown, area) && !covers(box, inner);
};
const describe = (layer, own) => {
  const owner = label(layer);
  for (const paint of shown(layer.fills)) {
    if (paint.type === "IMAGE") {
      if (paint.rotation || shown([paint.filters]).some((filters) => Object.values(filters).some((value) => value))) {
        approximate.push(`${owner} has an image fill with a rotation or filters`);
      }
      images.add(paint.imageHash);
    } else if (paint.type !== "SOLID" && !paint.type.startsWith("GRADIENT_")) {
      approximate.push(`${owner} is painted with ${paint.type}`);
    }
  }
  for (const effect of shown(layer.effects)) {
    const fine = effect.type === "LAYER_BLUR" || (effect.type === "DROP_SHADOW" && !effect.spread)
      || (own && effect.type === "BACKGROUND_BLUR");
    if (!fine) approximate.push(`${owner} has a ${effect.type}${effect.spread ? " with spread" : ""}`);
  }
  // A rectangle's stroke devup-mcp draws: solid or gradient, undashed, and
  // mitred where it reaches past the corners.
  const strokes = shown(layer.strokes);
  if (strokeReaches(layer)) {
    const rectangular = ["FRAME", "RECTANGLE", "COMPONENT", "COMPONENT_SET", "INSTANCE", "SECTION"].includes(layer.type);
    const painted = strokes.every((paint) => paint.type === "SOLID" || paint.type.startsWith("GRADIENT_"));
    const dashed = Array.isArray(layer.dashPattern) && layer.dashPattern.length > 0;
    const mitred = layer.strokeAlign === "INSIDE" || !layer.strokeJoin || layer.strokeJoin === "MITER";
    if (!(rectangular && painted && !dashed && mitred)) approximate.push(`${owner}'s stroke reaches it`);
  }
  const weight = (side) => (typeof layer[side] === "number" ? layer[side] : typeof layer.strokeWeight === "number" ? layer.strokeWeight : 0);
  return {
    transform: plain(layer.absoluteTransform), width: layer.width, height: layer.height, radii: radii(layer),
    fills: plain(shown(layer.fills)), opacity: layer.opacity === undefined ? 1 : layer.opacity,
    blendMode: layer.blendMode || null, clips: !!layer.clipsContent, effects: plain(shown(layer.effects)),
    strokes: plain(strokes), strokeAlign: layer.strokeAlign || null,
    strokeWeights: ["strokeTopWeight", "strokeRightWeight", "strokeBottomWeight", "strokeLeftWeight"].map(weight),
  };
};
const picture = async (layer) => {
  const bytes = await layer.exportAsync(settings(true, layer === node));
  // A mask draws nothing itself, so Figma gives it no render bounds; its
  // export covers its box.
  return { data: figma.base64Encode(bytes), area: layer.absoluteRenderBounds || layer.absoluteBoundingBox, blendMode: layer.blendMode || null, size: pngSize(bytes) };
};
const maskOf = async (member) => {
  const mask = masks.get(member);
  if (!mask) return null;
  // A mask draws nothing itself: Figma gives it no render bounds, and an export
  // by them is empty. Exported by its box, it shows its shape.
  const bytes = await mask.exportAsync({ ...settings(true, false), useAbsoluteBounds: true });
  return { data: figma.base64Encode(bytes), area: mask.absoluteBoundingBox, luminance: mask.maskType === "LUMINANCE" };
};

const layers = [];
const content = {};
const imageData = {};
let size;
// A part's export or an image read can fail where the canvas export does not
// (Figma refuses data it has not loaded while the file is out of sight); the
// answer is then the canvas one, never a refusal.
try {
  for (const layer of chain) {
    const described = describe(layer, false);
    const mask = await maskOf(layer);
    if (mask) described.mask = mask;
    layers.push(described);
  }
  // A background blur in the node's own export samples the layers behind it,
  // which get_screenshot leaves out; draw the node from its own paints and its
  // children's exports instead.
  const blursBehind = (layer) => shown(layer.effects).some((effect) => effect.type === "BACKGROUND_BLUR");
  const sampled = below.length > 0 && (blursBehind(node) || ("findOne" in node && !!node.findOne(blursBehind)));
  if (sampled) {
    const childBlur = "children" in node && node.children.some((child) => blursBehind(child) || ("findOne" in child && !!child.findOne(blursBehind)));
    if (!("children" in node) || childBlur || shown(node.strokes).length) {
      approximate.push(`${label(node)} blurs what lies behind it`);
    } else {
      content.own = describe(node, true);
      content.children = [];
      for (const child of node.children) {
        if (child.visible === false || child.isMask || !child.absoluteRenderBounds) continue;
        const exported = await picture(child);
        content.children.push({ data: exported.data, area: exported.area, blendMode: exported.blendMode });
      }
      size = [Math.ceil(area.width * scale - 1e-6), Math.ceil(area.height * scale - 1e-6)];
    }
  } else if (shapesMask) {
    size = [Math.ceil(area.width * scale - 1e-6), Math.ceil(area.height * scale - 1e-6)];
  } else {
    const exported = await picture(node);
    content.picture = { data: exported.data, area: exported.area, blendMode: exported.blendMode };
    size = exported.size;
  }
  const mask = await maskOf(node);
  if (mask) content.mask = mask;
  if (size && size[0] * size[1] > 8e6) approximate.push(`the area is ${size[0]}x${size[1]} pixels`);

  // An image devup-mcp does not decode - not a PNG, JPEG or GIF, an edge past
  // what the plugin exports, or mostly metadata - the plugin's UI decodes as a
  // browser does and hands back as a PNG, laid out at the original's size.
  // Others go as they are, so their pixels stay what they were.
  const normalize = globalThis.__devupNormalizeImage;
  let imageBytes = 0;
  for (const hash of images) {
    const image = hash && figma.getImageByHash(hash);
    let bytes = image && await image.getBytesAsync();
    if (!bytes) {
      approximate.push(`the image ${hash} could not be read`);
      continue;
    }
    let laidOut = null;
    if (typeof normalize === "function") {
      const { width, height } = await image.getSizeAsync();
      const starts = (...values) => values.every((value, at) => bytes[at] === value);
      const decoded = starts(0x89, 0x50, 0x4e, 0x47) || starts(0xff, 0xd8) || starts(0x47, 0x49, 0x46);
      if (!decoded || Math.max(width, height) > 8192 || bytes.length > 4 * width * height + 1024 * 1024) {
        const png = await normalize(bytes, 8192);
        if (png) {
          bytes = png;
          laidOut = { width, height };
        }
      }
    }
    imageBytes += bytes.length;
    imageData[hash] = laidOut ? { data: figma.base64Encode(bytes), ...laidOut } : figma.base64Encode(bytes);
  }
  if (imageBytes > 64 * 1024 * 1024) approximate.push(`its ancestors' images are ${imageBytes} bytes`);
} catch (error) {
  approximate.push(`a part could not be exported: ${error instanceof Error ? error.message : String(error)}`);
}

const reaching = `${intruders.slice(0, 5).join(", ")}${intruders.length > 5 ? ` and ${intruders.length - 5} more` : ""}`;
if (approximate.length > 0) return held(await canvasAnswer(approximation(approximate, reaching)));
// The canvas export rides along for devup-mcp to fall back on if composing fails.
return held(await canvasAnswer({
  reaching,
  compose: { area, scale, width: size[0], height: size[1], page: pageColour, layers, node: content, images: imageData },
}));
