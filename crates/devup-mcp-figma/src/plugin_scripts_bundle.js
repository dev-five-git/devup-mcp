(function () {
"use strict";
/** source: assets.js */
async function assets(p) {
// Figma's own MCP runs scripts with hidden layers inside instances left out
// of `children`, `findAll` and `getNodeByIdAsync`; a plugin sees them. Set
// here so the direct path and the Devup Bridge read the same document.
figma.skipInvisibleInstanceChildren = false;
const options = p.asset;
const DEVUP_MAX_LARGE_VALUE_BYTES = 16 * 1024 * 1024;
// A fragment's text is the chunk in base64, four thirds of it, inside a
// wrapper of some three hundred bytes, and the Figma MCP cuts a text result
// at 20,480 UTF-8 bytes (measured; see fast_snapshot.js). Twelve KiB comes
// back as 16.7 KB of text, with room to spare; eight KiB was 11.2 KB, and
// took a third more calls for the same value.
const DEVUP_LARGE_VALUE_CHUNK_BYTES = 12288;

function devupUtf8Encode(value) {
  const bytes = [];
  for (let index = 0; index < value.length; index += 1) {
    let codePoint = value.charCodeAt(index);
    if (codePoint >= 0xd800 && codePoint <= 0xdbff) {
      const next = index + 1 < value.length ? value.charCodeAt(index + 1) : 0;
      if (next >= 0xdc00 && next <= 0xdfff) {
        codePoint = 0x10000 + ((codePoint - 0xd800) << 10) + (next - 0xdc00);
        index += 1;
      } else codePoint = 0xfffd;
    } else if (codePoint >= 0xdc00 && codePoint <= 0xdfff) codePoint = 0xfffd;
    if (codePoint < 0x80) bytes.push(codePoint);
    else if (codePoint < 0x800) {
      bytes.push(0xc0 | (codePoint >> 6), 0x80 | (codePoint & 0x3f));
    } else if (codePoint < 0x10000) {
      bytes.push(
        0xe0 | (codePoint >> 12),
        0x80 | ((codePoint >> 6) & 0x3f),
        0x80 | (codePoint & 0x3f),
      );
    } else {
      bytes.push(
        0xf0 | (codePoint >> 18),
        0x80 | ((codePoint >> 12) & 0x3f),
        0x80 | ((codePoint >> 6) & 0x3f),
        0x80 | (codePoint & 0x3f),
      );
    }
  }
  return new Uint8Array(bytes);
}

function devupSha256(bytes) {
  const constants = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
    0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
    0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
  ];
  const paddedLength = Math.ceil((bytes.length + 9) / 64) * 64;
  const padded = new Uint8Array(paddedLength);
  padded.set(bytes);
  padded[bytes.length] = 0x80;
  const bitLength = bytes.length * 8;
  for (let index = 0; index < 8; index += 1) {
    padded[paddedLength - 1 - index] = Math.floor(bitLength / 2 ** (index * 8)) & 0xff;
  }
  const hash = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
    0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ];
  const rotate = (value, bits) => (value >>> bits) | (value << (32 - bits));
  for (let offset = 0; offset < padded.length; offset += 64) {
    const words = new Uint32Array(64);
    for (let index = 0; index < 16; index += 1) {
      const start = offset + index * 4;
      words[index] =
        (padded[start] << 24) |
        (padded[start + 1] << 16) |
        (padded[start + 2] << 8) |
        padded[start + 3];
    }
    for (let index = 16; index < 64; index += 1) {
      const s0 = rotate(words[index - 15], 7) ^ rotate(words[index - 15], 18) ^ (words[index - 15] >>> 3);
      const s1 = rotate(words[index - 2], 17) ^ rotate(words[index - 2], 19) ^ (words[index - 2] >>> 10);
      words[index] = (words[index - 16] + s0 + words[index - 7] + s1) >>> 0;
    }
    let [a, b, c, d, e, f, g, h] = hash;
    for (let index = 0; index < 64; index += 1) {
      const s1 = rotate(e, 6) ^ rotate(e, 11) ^ rotate(e, 25);
      const choice = (e & f) ^ (~e & g);
      const first = (h + s1 + choice + constants[index] + words[index]) >>> 0;
      const s0 = rotate(a, 2) ^ rotate(a, 13) ^ rotate(a, 22);
      const majority = (a & b) ^ (a & c) ^ (b & c);
      const second = (s0 + majority) >>> 0;
      h = g; g = f; f = e; e = (d + first) >>> 0;
      d = c; c = b; b = a; a = (first + second) >>> 0;
    }
    for (const [index, value] of [a, b, c, d, e, f, g, h].entries()) {
      hash[index] = (hash[index] + value) >>> 0;
    }
  }
  return hash.map((value) => value.toString(16).padStart(8, "0")).join("");
}

function devupLargeValueDescriptor(nodeId, field, serialized) {
  const bytes = devupUtf8Encode(JSON.stringify(serialized));
  if (bytes.length > DEVUP_MAX_LARGE_VALUE_BYTES) {
    return { $truncated: "max-large-value-bytes", byteLength: bytes.length };
  }
  return {
    $largeValue: {
      nodeId,
      field,
      byteLength: bytes.length,
      sha256: devupSha256(bytes),
      cursor: { nextOffset: 0, maxChunkBytes: DEVUP_LARGE_VALUE_CHUNK_BYTES },
    },
  };
}
;
// Read-only recovery. Never clone, detach, crop a parent or manufacture a
// substitute: the returned geometry must belong to this exact subtree.
async function devupLookupAssetNode(id) {
  try {
    const node = await figma.getNodeByIdAsync(id);
    if (node) return node;
  } catch (error) {
    if (!id.startsWith("I") || !id.includes(";")) throw error;
    try { return await walk(); }
    catch (fallback) { throw new Error(`${error.message || error}; nested lookup fallback: ${fallback.message || fallback}`); }
  }
  if (!id.startsWith("I") || !id.includes(";")) return null;
  return walk();

  async function walk() {
    const root = await figma.getNodeByIdAsync(id.slice(1).split(";")[0]);
    const pending = root ? [root] : [];
    const seen = new Set();
    while (pending.length) {
      const node = pending.pop();
      if (node.id === id) return node;
      if (seen.has(node.id)) continue;
      seen.add(node.id);
      if (seen.size > 10000) throw new Error("nested lookup exceeds 10000 nodes");
      if (Array.isArray(node.children)) pending.push(...node.children);
    }
    throw new Error(`nested node ${id} not found in its instance`);
  }
}

function devupGeometrySvg(root) {
  const escape = value => String(value).replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&apos;"}[c]));
  const finite = value => typeof value === "number" && Number.isFinite(value);
  const fail = (node, reason) => { throw new Error(`geometry fallback unsupported: ${node.type || "unknown"} ${node.id || ""}: ${reason}`); };
  let geometryCount = 0, visited = 0;
  function paintPaths(node, paths, paints) {
    if ((paints !== undefined && !Array.isArray(paints)) || (paints === undefined && Array.isArray(paths) && paths.length)) fail(node, "unresolved paint requires native export");
    const visible = Array.isArray(paints) ? paints.filter(p => p.visible !== false) : [];
    if (!visible.length) return "";
    if (!Array.isArray(paths) || !paths.length) fail(node, "paint has no rendered geometry");
    return visible.map(paint => {
      if (paint.type !== "SOLID" || (paint.blendMode && paint.blendMode !== "NORMAL")) fail(node, "paint type or blending requires native export");
      const color = paint.color;
      if (!color || ![color.r, color.g, color.b].every(c => finite(c) && c >= 0 && c <= 1)) fail(node, "invalid solid color");
      const opacity = paint.opacity === undefined ? 1 : paint.opacity;
      if (!finite(opacity) || opacity < 0 || opacity > 1) fail(node, "invalid paint opacity");
      // Paths of one winding rule are painted together, preserving holes and
      // overlap within the rendered geometry. Preserve floating-point colors.
      return ["NONZERO", "EVENODD"].map(rule => {
        const matching = paths.filter(path => path.windingRule === rule);
        if (!matching.length) return "";
        for (const path of matching) {
          if (typeof path.data !== "string" || !path.data.trim()) fail(node, "empty path");
          geometryCount++;
        }
        return `<path d="${escape(matching.map(p => p.data).join(" "))}" fill="rgb(${color.r * 100}%,${color.g * 100}%,${color.b * 100}%)" fill-opacity="${opacity}" fill-rule="${rule === "EVENODD" ? "evenodd" : "nonzero"}"/>`;
      }).join("");
    }).join("");
  }
  function render(node, isRoot) {
    if (++visited > 10000) fail(node, "subtree exceeds 10000 nodes");
    if (node.isMask) fail(node, "mask requires native export");
    if (node.visible === false || node.opacity === 0) return "";
    if (node.clipsContent) fail(node, "clipping requires native export");
    if (Array.isArray(node.effects) && node.effects.some(e => e.visible !== false)) fail(node, "effect requires native export");
    if (node.blendMode && !["NORMAL", "PASS_THROUGH"].includes(node.blendMode)) fail(node, "layer blending requires native export");
    const opacity = node.opacity === undefined ? 1 : node.opacity;
    if (!finite(opacity) || opacity < 0 || opacity > 1) fail(node, "invalid layer opacity");
    let transform = "";
    if (!isRoot) {
      const t = node.relativeTransform;
      if (!Array.isArray(t) || t.length !== 2 || !t.every(row => Array.isArray(row) && row.length === 3 && row.every(finite))) fail(node, "missing relative transform");
      transform = ` transform="matrix(${t[0][0]} ${t[1][0]} ${t[0][1]} ${t[1][1]} ${t[0][2]} ${t[1][2]})"`;
    }
    let body = "";
    if (["GROUP", "FRAME", "INSTANCE", "COMPONENT"].includes(node.type)) {
      if ((Array.isArray(node.fills) && node.fills.some(p => p.visible !== false)) || (Array.isArray(node.strokes) && node.strokes.some(p => p.visible !== false))) fail(node, "container paint requires native export");
      if (!Array.isArray(node.children)) fail(node, "missing children");
      const children = node.itemReverseZIndex === true ? [...node.children].reverse() : node.children;
      body = children.map(child => render(child, false)).join("");
    } else {
      if (!["VECTOR", "BOOLEAN_OPERATION", "RECTANGLE", "ELLIPSE", "POLYGON", "STAR", "LINE"].includes(node.type)) fail(node, "node type requires native export");
      // Plugin API strokeGeometry is centered even for INSIDE/OUTSIDE strokes.
      // Using it there would silently change the shape.
      if (Array.isArray(node.strokes) && node.strokes.some(p => p.visible !== false) && node.strokeAlign !== "CENTER") fail(node, "non-center stroke alignment requires native export");
      if (Array.isArray(node.dashPattern) && node.dashPattern.length) fail(node, "dashed stroke requires native export");
      for (const paths of [node.fillGeometry, node.strokeGeometry]) {
        if (paths !== undefined && (!Array.isArray(paths) || paths.some(p => !["NONZERO", "EVENODD"].includes(p.windingRule)))) fail(node, "unknown winding rule");
      }
      // Baked geometry includes boolean operations and centered stroke outlines.
      // Never approximate those using a bounding box.
      body = paintPaths(node, node.fillGeometry, node.fills) + paintPaths(node, node.strokeGeometry, node.strokes);
    }
    return `<g opacity="${opacity}"${transform}>${body}</g>`;
  }
  const bounds = root.absoluteRenderBounds, t = root.absoluteTransform;
  // Native exports include stroke extents. Convert rendered bounds back to
  // local space only when this is exact; rotated/sheared bounds lose that info.
  if (!bounds || ![bounds.x, bounds.y, bounds.width, bounds.height].every(finite) || bounds.width <= 0 || bounds.height <= 0) fail(root, "missing render bounds");
  if (!Array.isArray(t) || t.length !== 2 || !t.every(row => Array.isArray(row) && row.length === 3 && row.every(finite)) || t[0][1] !== 0 || t[1][0] !== 0 || t[0][0] <= 0 || t[1][1] <= 0) fail(root, "root rotation, reflection or shear requires native export");
  const x = (bounds.x - t[0][2]) / t[0][0], y = (bounds.y - t[1][2]) / t[1][1];
  const width = bounds.width / t[0][0], height = bounds.height / t[1][1];
  const body = render(root, true);
  if (!geometryCount) fail(root, "no painted geometry");
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="${x} ${y} ${width} ${height}">${body}</svg>`;
}

async function devupRenderAsset(node, settings, nativeBudgetMs = 85000, observe = () => {}) {
  const began = Date.now();
  let timer;
  try {
    const result = await Promise.race([
      node.exportAsync(settings),
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`DEVUP_ASSET_TIMEOUT: native export exceeded ${nativeBudgetMs} ms`)), nativeBudgetMs); }),
    ]);
    observe("native-export", Date.now() - began);
    return result;
  }
  catch (nativeError) {
    observe("native-export", Date.now() - began, String(nativeError.message || nativeError).slice(0, 500));
    if (settings.format !== "SVG_STRING") throw nativeError;
    const fallbackBegan = Date.now();
    try {
      const svg = devupGeometrySvg(node);
      observe("geometry-fallback", Date.now() - fallbackBegan);
      return svg;
    }
    catch (fallbackError) { throw new Error(`native export: ${nativeError.message || nativeError}; ${fallbackError.message || fallbackError}`); }
  } finally { clearTimeout(timer); }
}
;

const started = Date.now();
let stage = "lookup";
let nodeType = "unknown";
const timeoutMs = Math.min(600000, Math.max(1000, Number(options.timeoutMs) || 90000));
const exportDiagnostics = { method: "native", stages: [] };
function observe(stage, elapsedMs, nativeError) {
  exportDiagnostics.stages.push({ stage, elapsedMs });
  if (nativeError) exportDiagnostics.nativeError = nativeError;
  if (stage === "geometry-fallback") exportDiagnostics.method = "geometry";
}
async function timed(work) {
  let timer;
  try {
    return await Promise.race([work(), new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`DEVUP_ASSET_TIMEOUT: ${stage} exceeded ${timeoutMs} ms asset budget`)), Math.max(0, timeoutMs - (Date.now() - started)));
    })]);
  } finally { clearTimeout(timer); }
}

function describe(error) {
  const message = error && typeof error.message === "string" ? error.message : String(error);
  const editor = typeof figma.editorType === "string" ? ` (editor: ${figma.editorType})` : "";
  return `${stage} failed after ${Date.now() - started} ms${editor} (node: ${options.nodeId}, type: ${nodeType}): ${message}`.slice(0, 500);
}

function failed(errorCode, errorDetail) {
  if (errorCode && exportDiagnostics.renderInfo) {
    exportDiagnostics.renderInfo.attemptedRepresentation = exportDiagnostics.renderInfo.representation;
    exportDiagnostics.renderInfo.representation = "not-exported";
    exportDiagnostics.renderInfo.renderedAs = "not-exported";
    exportDiagnostics.renderInfo.effectsBakedIntoAsset = false;
    exportDiagnostics.renderInfo.fidelity = "unverified";
  }
  return {
    kind: "devupAssetExport",
    fileKey: figma.fileKey || "",
    version: options.version,
    assetId: options.assetId,
    nodeId: options.nodeId,
    field: options.field,
    imageHash: options.imageHash,
    format: options.format,
    scale: options.scale,
    renderMode,
    usageAspectRatio: options.usageAspectRatio ?? null, maxWidth: options.maxWidth ?? null, srcset: options.srcset === true, visibleScopeNodeId: options.visibleScopeNodeId ?? null,
    status: "failed",
    byteLength: null,
    sha256: null,
    errorCode,
    exportDiagnostics,
    ...(errorDetail ? { errorDetail } : {}),
  };
}

const original = typeof options.field === "string" && options.field.startsWith("$original-image/fills/");
const renderMode = options.renderMode === undefined ? (original ? "fill" : ["svg", "pdf"].includes(options.format) ? "node" : "original-source") : options.renderMode;
let effectiveMode = ["auto", "original-source"].includes(renderMode) ? (renderMode === "auto" ? "node" : "fill") : renderMode;
function renderInfo(node, paint) {
  const finite = n => typeof n === "number" && Number.isFinite(n) && n > 0;
  const effects = Array.isArray(node.effects) ? node.effects : null;
  const ancestors = [];
  const boundedName = name => [...name].slice(0, 128).join("");
  const layerHierarchy = [typeof node.name === "string" ? boundedName(node.name) : "layer"];
  const seen = new Set();
  let branch = node;
  for (let parent = node.parent; parent && ancestors.length < 32 && !seen.has(parent); parent = parent.parent) {
    seen.add(parent);
    if (layerHierarchy.length < 4 && typeof parent.name === "string" && parent.name.length && !["PAGE", "DOCUMENT"].includes(parent.type)) {
      layerHierarchy.push(boundedName(parent.name));
    }
    ancestors.push({ type: parent.type || "unknown", clipsContent: parent.clipsContent ?? null,
      isMask: parent.isMask ?? null, effects: Array.isArray(parent.effects) ? parent.effects : null,
      opacity: parent.opacity ?? null, blendMode: parent.blendMode ?? null, visible: parent.visible ?? null,
      siblingMaskPresent: Array.isArray(parent.children) ? parent.children.some(n => n !== branch && n.isMask === true) : null });
    branch = parent;
  }
  return {
    sourceKind: paint ? "image-fill" : options.imageHash ? "image-node" : "vector-node",
    renderedAs: effectiveMode === "fill" ? "raw-fill" : "node-render",
    representation: effectiveMode === "fill" ? "original-fill" : "native-node",
    layerHierarchy: layerHierarchy.reverse(),
    scope: effectiveMode === "fill" ? "selected-paint-original-bytes" : "exact-node-subtree",
    exportSettings: effectiveMode === "node" ? { contentsOnly: true, useAbsoluteBounds: false } : null,
    requestedScale: options.scale,
    scaleSemantics: effectiveMode === "fill" ? "intrinsic-original-bytes-no-resize"
      : ["png", "jpg"].includes(options.format) ? "native-node-SCALE-constraint" : "not-applied-to-vector-document",
    nodeSize: finite(node.width) && finite(node.height) ? {width:node.width, height:node.height, aspectRatio:node.width/node.height} : null,
    outputSize: null, dimensionEvidence: "unknown-until-byte-inspection", aspectRatioComparison: "unknown",
    scaleMode: paint ? paint.scaleMode ?? null : null,
    imageTransform: paint ? paint.imageTransform ?? null : null,
    // A transform matrix is not a visible-area percentage. Rotation, fit,
    // clipping and source geometry are needed to determine visible coverage.
    visibleSourceRatio: null, visibleSourceRatioEvidence: "not-computed",
    effects,
    extraFills: Array.isArray(node.fills) ? Math.max(0, node.fills.filter(p => p.visible !== false).length - (paint ? 1 : 0)) : null,
    effectsBakedIntoAsset: effectiveMode === "node" && ["png", "jpg"].includes(options.format) && effects !== null,
    bakedEffects: effectiveMode === "node" && ["png", "jpg"].includes(options.format) && effects ? effects.filter(e => e.visible !== false) : [],
    instanceOverride: "unknown", instanceOverrideEvidence: "no-component-paint-comparison",
    sourceProperties: { scaleMode: paint ? paint.scaleMode ?? null : null,
      imageTransform: paint ? paint.imageTransform ?? null : null,
      rotation: paint ? paint.rotation ?? null : null, filters: paint ? paint.filters ?? null : null,
      opacity: paint ? paint.opacity ?? null : null, effects,
      blurEffects: effects ? effects.filter(e => e.visible !== false && String(e.type).includes("BLUR")) : null,
      isMask: node.isMask ?? null, clipsContent: node.clipsContent ?? null, ancestors },
    unappliedProperties: effectiveMode === "fill" ? ["scaleMode", "imageTransform", "rotation", "filters", "paintOpacity", "nodeGeometry", "nodeTransforms", "nodeEffectsIncludingBlur", "nodeAndAncestorMasks", "ancestorClipsAndEffects"] : [],
    warnings: effectiveMode === "fill" ? ["original-fill-properties-unapplied", "aspect-ratio-unknown"]
      : ["ancestor-canvas-appearance-unverified", "aspect-ratio-unknown"],
    fidelity: "unverified",
  };
}

// Unknown properties fail closed. Identity alone cannot establish canvas
// equality, and auto never infers source dimensions from the transform.
function rawSafety(node, paint, info) {
  const reasons = [];
  const require = (condition, reason) => { if (!condition) reasons.push(reason); };
  const identity = t => Array.isArray(t) && t.length === 2 && t.every((r,i) => Array.isArray(r) && r.length === 3 && r.every((v,j) => v === [[1,0,0],[0,1,0]][i][j]));
  const unrotated = t => Array.isArray(t) && t.length === 2 && t.every(r => Array.isArray(r) && r.length === 3 && r.every(Number.isFinite)) && t[0][0] === 1 && t[1][1] === 1 && t[0][1] === 0 && t[1][0] === 0 && Number.isInteger(t[0][2]) && Number.isInteger(t[1][2]);
  require(options.transport === "bridge", "raw-requires-bridge");
  require(options.scale === 1 && ["png","jpg"].includes(options.format), "raw-requires-unscaled-raster");
  require(!!paint && paint.visible === true && paint.opacity === 1 && paint.blendMode === "NORMAL", "paint-compositing-or-unknown");
  require(paint && paint.scaleMode === "FILL" && identity(paint.imageTransform) && paint.rotation === 0, "paint-transform-or-unknown");
  require(paint && paint.filters && Object.values(paint.filters).every(v => v === 0), "image-filters-or-unknown");
  require(Array.isArray(node.fills) && node.fills.length === 1, "multiple-fills-or-unknown");
  require(node.type === "RECTANGLE" && node.cornerRadius === 0 && Array.isArray(node.strokes) && node.strokes.length === 0 && (!node.children || node.children.length === 0), "node-geometry-or-unknown");
  // RectangleNode has no clipsContent in the Plugin API. Its absence is
  // non-applicable for this geometry, rather than missing Frame evidence.
  require(Array.isArray(node.effects) && node.effects.length === 0 && node.isMask === false && (node.clipsContent === undefined || node.clipsContent === false) && node.opacity === 1 && node.visible === true && node.blendMode === "NORMAL", "node-effects-mask-compositing-or-unknown");
  require(unrotated(node.absoluteTransform) && unrotated(node.relativeTransform), "node-transform-or-unknown");
  require(info.sourceProperties.ancestors.length > 0 && info.sourceProperties.ancestors.some(p => ["PAGE","DOCUMENT"].includes(p.type)), "ancestor-chain-incomplete");
  require(info.sourceProperties.ancestors.every(p => p.siblingMaskPresent === false), "ancestor-sibling-mask-or-unknown");
  for (const p of info.sourceProperties.ancestors.filter(p => !["PAGE","DOCUMENT"].includes(p.type))) {
    require(p.clipsContent === false && p.isMask === false && p.siblingMaskPresent === false && Array.isArray(p.effects) && p.effects.length === 0 && p.opacity === 1 && p.visible === true && ["NORMAL","PASS_THROUGH"].includes(p.blendMode), "ancestor-dependency-or-unknown");
  }
  return { eligible: reasons.length === 0, reasons: [...new Set(reasons)], evidence: "explicit-plugin-properties; intrinsic-dimensions-and-codec-required" };
}

function overrideEvidence(node, info) {
  let count = 0;
  for (let parent = node; parent && count++ < 32; parent = parent.parent) {
    if (parent.type === "INSTANCE" && Array.isArray(parent.overrides)) {
      const evidence = parent.overrides.find(o => o.id === node.id && Array.isArray(o.overriddenFields) && o.overriddenFields.includes("fills"));
      if (evidence) { info.instanceOverride = true; info.instanceOverrideEvidence = { kind: "figma-instance-overrides", instanceId: parent.id, nodeId: node.id, overriddenFields: ["fills"] }; return; }
    }
    if (["PAGE","DOCUMENT"].includes(parent.type)) break;
  }
}
try {
  if (!["original-source", "fill", "node", "visible", "auto"].includes(renderMode)) return failed("DEVUP_ASSET_RENDER_MODE_UNSUPPORTED");
  if (!original && renderMode === "fill" &&
      (!String(options.field).startsWith("fills/") || !["png", "jpg", "webp"].includes(options.format) || options.scale !== 1)) {
    return failed("DEVUP_ASSET_FILL_SETTINGS_UNSUPPORTED", "Original fill requires fills/N, png or jpg matching its codec, and scale 1; no resizing or transcoding.");
  }
  // Original uploads are a different representation from node renditions.
  // Only the local bridge opts in: the remote MCP's bounded text envelope
  // cannot carry arbitrary original bytes, and its file writer is not a
  // general binary transport. Never silently return a node PNG instead.
  if ((original || ["fill", "original-source"].includes(renderMode)) && options.transport !== "bridge") {
    return failed(original ? "DEVUP_ORIGINAL_IMAGE_REQUIRES_BRIDGE" : "DEVUP_ASSET_FILL_REQUIRES_BRIDGE");
  }
  const node = await timed(() => devupLookupAssetNode(options.nodeId));
  observe("lookup", Date.now() - started);
  nodeType = node && node.type || "unknown";
  if (!node || (["node", "visible"].includes(renderMode) && typeof node.exportAsync !== "function")) {
    return failed("DEVUP_ASSET_UNSUPPORTED_BY_UPSTREAM", describe(new Error(node ? "node has no exportAsync" : "node not found")));
  }
  let selectedPaint = null;
  if (original || (typeof options.field === "string" && options.field.startsWith("fills/"))) {
    const index = Number(options.field.slice(original ? "$original-image/fills/".length : "fills/".length));
    const fills = "fills" in node && Array.isArray(node.fills) ? node.fills : [];
    const paint = Number.isInteger(index) ? fills[index] : null;
    const imageHash = paint && paint.type === "IMAGE" ? paint.imageHash || paint.imageRef : null;
    if (!paint || imageHash !== options.imageHash) {
      return failed("DEVUP_ASSET_SOURCE_CHANGED");
    }
    selectedPaint = paint;
  } else if (options.field !== "node") {
    return failed("DEVUP_ASSET_FIELD_UNSUPPORTED");
  }
  if (renderMode === "original-source" && (!selectedPaint || !options.imageHash)) return failed("DEVUP_ORIGINAL_SOURCE_UNAVAILABLE", "No IMAGE fill source exists for this selection. Choose an IMAGE fill or explicitly renderMode node; vector assets should retain SVG.");
  let captureNode = node;
  if (renderMode === "visible") {
    const scopeId = options.visibleScopeNodeId;
    if (!scopeId) return failed("DEVUP_VISIBLE_CONTEXT_UNAVAILABLE", "Recollect the parent frame and provide visibleScopeNodeId.");
    const seen = new Set();
    while (captureNode && captureNode.id !== scopeId && !seen.has(captureNode) && seen.size < 64) {
      seen.add(captureNode); captureNode = captureNode.parent;
    }
    if (!captureNode || captureNode.id !== scopeId || ["PAGE","DOCUMENT"].includes(captureNode.type) || typeof captureNode.exportAsync !== "function") return failed("DEVUP_VISIBLE_SCOPE_UNSUPPORTED", "The selected capture scope is not an exportable ancestor of the requested node.");
  }
  exportDiagnostics.renderInfo = renderInfo(node, selectedPaint);
  if (renderMode === "visible") {
    const info = exportDiagnostics.renderInfo;
    info.representation = "native-visible"; info.renderedAs = "visible-scoped-capture";
    info.captureNodeId = captureNode.id;
    info.captureBox = {width:captureNode.width, height:captureNode.height, aspectRatio:captureNode.width/captureNode.height};
    info.contextScope = "selected-ancestor-subtree";
    info.captureEffects = Array.isArray(captureNode.effects) ? captureNode.effects : null;
    info.requestedNodeEffects = info.effects;
    info.effects = info.captureEffects;
    info.sourceProperties.captureEffects = info.captureEffects;
    info.visibleContext = {requestedNodeId:node.id,captureNodeId:captureNode.id,
      included:["selected-scope-children","selected-scope-clipping","selected-scope-masks","selected-scope-rounding"],
      outsideScope:"unknown; ancestors, page backdrop and external layers are not included",canvasReferenceCompared:false};
    info.warnings.push("visible-scoped-capture", "outside-capture-context-unverified");
  }
  overrideEvidence(node, exportDiagnostics.renderInfo);
  let observedSourceSize = null;
  let sizeTimer;
  if (selectedPaint) {
    try {
      const image = figma.getImageByHash(options.imageHash);
      const size = image && await Promise.race([image.getSizeAsync(), new Promise(resolve => {
        sizeTimer = setTimeout(() => resolve(null), 250);
      })]);
      observedSourceSize = size && size.width > 0 && size.height > 0 && Number.isFinite(size.width) && Number.isFinite(size.height)
        ? { ...size, aspectRatio: size.width / size.height } : null;
    } catch (_error) { /* Optional diagnostics never consume the native export budget. */ }
    finally { clearTimeout(sizeTimer); }
  }
  let autoBytes = null;
  if (renderMode === "auto") {
    const safety = rawSafety(node, selectedPaint, exportDiagnostics.renderInfo);
    if (safety.eligible) {
      try {
        const image = figma.getImageByHash(options.imageHash);
        const sourceSize = observedSourceSize;
        if (!sourceSize || sourceSize.width !== node.width || sourceSize.height !== node.height) safety.reasons.push("intrinsic-dimensions-mismatch-or-unknown");
        else {
          autoBytes = await timed(() => image.getBytesAsync());
          const codec = autoBytes[0] === 137 && autoBytes[1] === 80 && autoBytes[2] === 78 && autoBytes[3] === 71 ? "png" : autoBytes[0] === 255 && autoBytes[1] === 216 && autoBytes[2] === 255 ? "jpg" : "unknown";
          if (codec !== options.format) safety.reasons.push("source-codec-mismatch-or-unknown");
        }
      } catch (_error) { safety.reasons.push("source-inspection-unavailable"); }
    }
    safety.eligible = safety.reasons.length === 0;
    effectiveMode = safety.eligible ? "fill" : "node";
    exportDiagnostics.renderInfo = renderInfo(node, selectedPaint);
    overrideEvidence(node, exportDiagnostics.renderInfo);
    exportDiagnostics.renderInfo.autoDecision = { ...safety, selected: effectiveMode };
  }
  exportDiagnostics.renderInfo.sourceSize = observedSourceSize;
  exportDiagnostics.renderInfo.sourceSizeEvidence = observedSourceSize ? "Figma-image-getSizeAsync" : "unknown";

  if (original || effectiveMode === "fill") {
    exportDiagnostics.method = "original-fill";
    stage = "reading the original image";
    const image = figma.getImageByHash(options.imageHash);
    if (!image) return failed("DEVUP_ORIGINAL_IMAGE_NOT_FOUND");
    const bytes = autoBytes || await timed(() => image.getBytesAsync());
    if (bytes.length === 0 || bytes.length > 80 * 1024 * 1024) {
      return failed("DEVUP_ASSET_RESPONSE_TOO_LARGE", `${bytes.length} bytes; one answer carries 1 to 83886080`);
    }
    const starts = values => values.every((value, index) => bytes[index] === value);
    const mimeType = starts([137,80,78,71,13,10,26,10]) ? "image/png"
      : starts([255,216,255]) ? "image/jpeg"
      : starts([71,73,70,56]) ? "image/gif"
      : starts([82,73,70,70]) && bytes[8] === 87 && bytes[9] === 69 && bytes[10] === 66 && bytes[11] === 80 ? "image/webp"
      : null;
    if (!mimeType) return failed("DEVUP_ORIGINAL_IMAGE_CODEC_UNSUPPORTED");
    exportDiagnostics.renderInfo.originalMimeType = mimeType;
    if (!original && renderMode === "fill" && options.format !== "webp" && mimeType !== { png: "image/png", jpg: "image/jpeg" }[options.format]) {
      return failed("DEVUP_ASSET_FILL_CODEC_MISMATCH", `Original fill codec is ${mimeType}, but ${options.format} was requested; choose the matching format or renderMode node. No transcoding was performed.`);
    }
    if (!original) return { ...failed(null), status: "exported", mimeType,
      byteLength: bytes.length, sha256: devupSha256(bytes), data: figma.base64Encode(bytes) };
    const size = await timed(() => image.getSizeAsync());
    return {
      ...failed(null), status: "exported", kind: "devupOriginalImage",
      representation: "original-image-v1", format: null, scale: null,
      mimeType, width: size.width, height: size.height,
      byteLength: bytes.length, sha256: devupSha256(bytes), data: figma.base64Encode(bytes),
    };
  }

  // WebP is encoded by the server from this exact PNG; Figma has no native WebP export.
  const format = options.format === "webp" ? "PNG" : String(options.format || "").toUpperCase();
  if (!["PNG", "JPG", "SVG", "PDF"].includes(format)) {
    return failed("DEVUP_ASSET_FORMAT_UNSUPPORTED");
  }
  const scale = Number(options.scale ?? 2);
  if (!Number.isFinite(scale) || scale <= 0 || scale > 4) return failed("DEVUP_ASSET_SCALE_INVALID");
  // SVG is exported as a string and carried back inline. Figma's remote MCP
  // does not return a written `.svg` as an attachment the way it does a PNG,
  // so writing the file alone left the caller holding a descriptor and no
  // bytes at all, and every SVG request failed. SVG is text and small, so an
  // inline copy is bounded well under the text-response limit; anything
  // larger is reported rather than silently truncated.
  const inlineSvg = format === "SVG";
  const settings = { format: inlineSvg ? "SVG_STRING" : format, contentsOnly: true, useAbsoluteBounds: false };
  if (format === "PNG" || format === "JPG") {
    settings.constraint = { type: "SCALE", value: scale };
  }
  stage = "export";
  const remaining = Math.max(1, timeoutMs - (Date.now() - started));
  // Reserve up to five seconds for read-only geometry after a hung renderer.
  // A late native answer has no file writer and cannot replace the fallback.
  const nativeBudget = inlineSvg ? Math.max(1, remaining - Math.min(5000, Math.floor(remaining / 5))) : remaining;
  const nativeBegan = Date.now();
  // Image-filled nodes must use Figma's renderer for crop, filters, effects,
  // transforms and masks within this subtree. Never substitute raw fill bytes
  // or geometry on renderer failure. The canvas outside this node is unverified.
  const exported = await timed(() => renderMode === "visible" ? captureNode.exportAsync(settings)
    : selectedPaint ? node.exportAsync(settings) : devupRenderAsset(node, settings, nativeBudget, observe));
  if (selectedPaint) observe("native", Date.now() - nativeBegan);
  if (exportDiagnostics.method === "geometry") {
    exportDiagnostics.renderInfo.representation = "geometry-fallback";
    exportDiagnostics.renderInfo.renderedAs = "geometry-fallback";
    exportDiagnostics.renderInfo.effectsBakedIntoAsset = false;
    exportDiagnostics.renderInfo.warnings.push("geometry-fallback-not-native-render");
  }
  if (Date.now() - started >= timeoutMs) throw new Error(`DEVUP_ASSET_TIMEOUT: export exceeded ${timeoutMs} ms asset budget`);
  stage = "encode";
  const encodeBegan = Date.now();
  const svgText = inlineSvg && typeof exported === "string" ? exported : null;
  const bytes =
    svgText === null
      ? exported instanceof Uint8Array
        ? exported
        : new Uint8Array(exported)
      : devupUtf8Encode(svgText);
  if (bytes.length === 0 || bytes.length > 80 * 1024 * 1024) {
    return failed("DEVUP_ASSET_RESPONSE_TOO_LARGE", `${format} at scale ${scale} is ${bytes.length} bytes; one answer carries 1 to 83886080`);
  }
  const sha256 = devupSha256(bytes);
  observe("encode", Date.now() - encodeBegan);
  // figma.io is an extension of the remote MCP, not the native Plugin API.
  // The bridge returns JSON over its local socket and has no attachment
  // writer. Carry its bounded bytes inline, without exporting again for each
  // fragment; the Rust decoder still validates length, MIME type and hash.
  const hasFileWriter = figma.io && typeof figma.io.write === "function";
  // A PNG past what one attachment carries is not written here either.
  // Figma's remote MCP returns a written PNG as an attachment only up to
  // about a megabyte once base64-encoded: a 665 KB photograph came back, a
  // 950 KB one was written, reported exported, and never arrived - the
  // devup-ui landing page's hero. Past 768 KiB, which is exactly one MiB
  // encoded, it is announced and read back in fragments like a large SVG.
  const pngTooLargeToAttach = format === "PNG" && bytes.length > 768 * 1024;
  if (hasFileWriter && ((svgText !== null && bytes.length > 12 * 1024) || pngTooLargeToAttach)) {
    // An SVG past what one text response holds is not written here at all:
    // it is announced with its length and hash, and read back in fragments
    // through the large-value script, which re-exports it and slices — the
    // same transport a large node field takes. An illustration of fifty
    // vectors is 125 KB; before this it simply failed.
    return {
      kind: "devupAssetExport",
      fileKey: figma.fileKey || "",
      version: options.version,
      assetId: options.assetId,
      nodeId: options.nodeId,
      field: options.field,
      imageHash: options.imageHash,
      format: options.format,
      scale,
      renderMode,
    usageAspectRatio: options.usageAspectRatio ?? null, maxWidth: options.maxWidth ?? null, srcset: options.srcset === true, visibleScopeNodeId: options.visibleScopeNodeId ?? null,
      status: "chunked",
      byteLength: bytes.length,
      sha256,
      cursor: { nextOffset: 0, maxChunkBytes: DEVUP_LARGE_VALUE_CHUNK_BYTES },
      errorCode: null,
      exportDiagnostics,
    };
  }
  if (hasFileWriter) {
    figma.io.write(`devup-asset-${options.assetId.replace(/[^A-Za-z0-9_-]/g, "_")}.${format.toLowerCase()}`, bytes);
  }
  return {
    kind: "devupAssetExport",
    fileKey: figma.fileKey || "",
    version: options.version,
    assetId: options.assetId,
    nodeId: options.nodeId,
    field: options.field,
    imageHash: options.imageHash,
    format: options.format,
    scale,
    renderMode,
    usageAspectRatio: options.usageAspectRatio ?? null, maxWidth: options.maxWidth ?? null, srcset: options.srcset === true, visibleScopeNodeId: options.visibleScopeNodeId ?? null,
    status: "exported",
    byteLength: bytes.length,
    sha256,
    // MIME identifies inline bridge bytes or remote SVG text as a payload.
    mimeType: !hasFileWriter
      ? { PNG: "image/png", JPG: "image/jpeg", SVG: "image/svg+xml", PDF: "application/pdf" }[format]
      : svgText === null ? null : "image/svg+xml",
    ...(!hasFileWriter && svgText === null ? { data: figma.base64Encode(bytes) } : {}),
    text: svgText,
    errorCode: null,
    exportDiagnostics,
  };
} catch (error) {
  return failed(String(error.message || error).includes("DEVUP_ASSET_TIMEOUT:") ? "DEVUP_ASSET_TIMEOUT" : original || ["fill", "original-source"].includes(renderMode) ? "DEVUP_ORIGINAL_IMAGE_READ_FAILED" : "DEVUP_ASSET_EXPORT_FAILED", describe(error));
}

}

/** source: bridge_context.js */
async function bridgeContext(p) {
// Operation-time read: independent of asynchronous selection event delivery.
const page = figma.currentPage;
const selected = page.selection;
const ref = node => ({id: node.id, name: String(node.name).slice(0, 256), type: node.type});
return {kind: 'devup-bridge-context', currentPage: ref(page),
  selection: selected.slice(0, 20).map(ref), selectionCount: selected.length,
  zoom: Number.isFinite(figma.viewport?.zoom) ? figma.viewport.zoom : null,
  observedAt: Date.now(), lastKnownSelection: globalThis.__devupLastSelection ?? null,
  lastKnownPage: globalThis.__devupLastSelectionPage ?? null};

}

/** source: explore.js */
async function explore(p) {
// Figma's own MCP runs scripts with hidden layers inside instances left out
// of `children`, `findAll` and `getNodeByIdAsync`; a plugin sees them. Set
// here so the direct path and the Devup Bridge read the same document.
figma.skipInvisibleInstanceChildren = false;
const anchor = await figma.getNodeByIdAsync(p.nodeId);
if (!anchor) throw new Error("DEVUP_NODE_NOT_FOUND");

const options = p.explore;
const projectionLimit = Math.max(20, Math.min(
  400,
  Number.isInteger(options.projectionLimit) ? options.projectionLimit : 200,
));
const textPreviewLimit = Math.max(0, Math.min(
  500,
  Number.isInteger(options.textPreviewLimit) ? options.textPreviewLimit : 160,
));
const MAX_PROJECTION_JSON_CHARS = 14_000;
const MAX_NAME_CHARS = 240;
const MAX_BREADCRUMB_SEGMENTS = 12;

function compactName(value) {
  return typeof value === "string" ? value.slice(0, MAX_NAME_CHARS) : "";
}

const requestedAnchor = anchor;
const requestedPage = requestedAnchor.type === "PAGE";
let pagePeer = requestedAnchor;
let nearestSection = requestedAnchor.type === "SECTION" ? requestedAnchor : null;
while (!requestedPage && pagePeer.parent && pagePeer.parent.type !== "PAGE") {
  pagePeer = pagePeer.parent;
  if (!nearestSection && pagePeer.type === "SECTION") nearestSection = pagePeer;
}
const page = requestedAnchor.type === "PAGE" ? requestedAnchor : pagePeer.parent;
if (!page || page.type !== "PAGE") throw new Error("DEVUP_PAGE_NOT_FOUND");
await figma.setCurrentPageAsync(page);

function bounds(node) {
  const absolute = node.absoluteBoundingBox;
  const value = absolute || {
    x: typeof node.x === "number" ? node.x : 0,
    y: typeof node.y === "number" ? node.y : 0,
    width: typeof node.width === "number" ? node.width : 0,
    height: typeof node.height === "number" ? node.height : 0,
  };
  if (![value.x, value.y, value.width, value.height].every(Number.isFinite)) return null;
  return { x: value.x, y: value.y, width: value.width, height: value.height };
}

// Carry a small measured navigation summary into screen planning, before a
// full page export. Missing measurements remain unknown rather than guessed.
function navigationContentWidth(root) {
  const queue = [root];
  let visited = 0;
  let width = null;
  while (queue.length && visited++ < 400) {
    const node = queue.shift();
    if (("visible" in node) && node.visible === false) continue;
    const children = "children" in node ? node.children.filter((child) => !("visible" in child) || child.visible !== false) : [];
    if (/header|nav|gnb/i.test(node.name || "") && children.length >= 2
      && ("layoutMode" in node) && node.layoutMode === "HORIZONTAL") {
      const sizes = children.map(bounds);
      if (sizes.every(Boolean)) {
        const measured = sizes.reduce((sum, box) => sum + box.width, 0) + (Number(node.itemSpacing) || 0) * (sizes.length - 1);
        width = Math.max(width || 0, measured);
      }
    }
    queue.push(...children.slice(0, 80));
  }
  return width;
}

function navigationSummary(root) {
  const width = navigationContentWidth(root);
  return width === null ? {} : { navigationContentWidth: width };
}


function horizontalOverlap(left, right) {
  return Math.min(left.x + left.width, right.x + right.width) - Math.max(left.x, right.x);
}

function isScreenLike(node) {
  const box = bounds(node);
  return box
    && ["FRAME", "COMPONENT", "INSTANCE", "COMPONENT_SET"].includes(node.type)
    && box.width >= 240
    && box.width <= 1800
    && box.height >= 300
    && box.height <= 2000
    && box.width / Math.max(1, box.height) >= 0.25
    && box.width / Math.max(1, box.height) <= 2.5;
}

function textPreview(node) {
  if (textPreviewLimit === 0) return "";
  const values = [];
  const queue = [node];
  let visited = 0;
  while (queue.length && visited < 80 && values.join(" ").length < textPreviewLimit) {
    const candidate = queue.shift();
    visited += 1;
    if (candidate.type === "TEXT" && typeof candidate.characters === "string") {
      values.push(candidate.characters.replace(/\s+/gu, " ").trim());
    }
    if ("children" in candidate) queue.push(...candidate.children.slice(0, 40));
  }
  return values.filter(Boolean).join(" ").slice(0, textPreviewLimit);
}

function breadcrumb(node) {
  const names = [];
  let current = node;
  while (current && current.type !== "DOCUMENT" && names.length < MAX_BREADCRUMB_SEGMENTS) {
    const name = compactName(current.name);
    if (name) names.push(name);
    current = current.parent;
  }
  return names.reverse();
}

const scopeAnchor = requestedPage ? page : !isScreenLike(requestedAnchor) && nearestSection
  ? nearestSection
  : pagePeer;
const anchorBounds = bounds(scopeAnchor) || bounds(requestedAnchor);
if (!anchorBounds) throw new Error("DEVUP_NODE_BOUNDS_UNAVAILABLE");
const pageChildren = "children" in page ? page.children : [];
const eligible = pageChildren
  .map((node, pageChildIndex) => ({ node, pageChildIndex, bounds: bounds(node) }))
  .filter((entry) => entry.bounds)
  .filter((entry) => {
    if (requestedPage) return true;
    if (entry.node.id === scopeAnchor.id) return true;
    const verticallyNear = entry.bounds.y >= anchorBounds.y - 240
      && entry.bounds.y <= anchorBounds.y + 12000;
    return verticallyNear && horizontalOverlap(entry.bounds, anchorBounds) > 0;
  })
  .sort((left, right) =>
    left.bounds.y - right.bounds.y
      || left.bounds.x - right.bounds.x
      || left.node.id.localeCompare(right.node.id),
  );
const selected = scopeAnchor.type === "SECTION"
  ? eligible.filter((entry) => entry.node.id === scopeAnchor.id)
  : eligible.slice(0, projectionLimit);
const included = new Map(selected.map((entry) => [entry.node.id, entry]));
if (!included.has(pagePeer.id)) {
  included.set(pagePeer.id, { node: pagePeer, pageChildIndex: -1, bounds: bounds(pagePeer) });
}
if (!included.has(scopeAnchor.id)) {
  included.set(scopeAnchor.id, { node: scopeAnchor, pageChildIndex: -1, bounds: bounds(scopeAnchor) });
}
if (!included.has(requestedAnchor.id)) {
  included.set(requestedAnchor.id, {
    node: requestedAnchor,
    pageChildIndex: -1,
    bounds: bounds(requestedAnchor),
  });
}
let requiredAncestor = requestedPage ? null : requestedAnchor.parent;
while (requiredAncestor && requiredAncestor.type !== "PAGE") {
  if (!included.has(requiredAncestor.id)) {
    included.set(requiredAncestor.id, {
      node: requiredAncestor,
      pageChildIndex: -1,
      bounds: bounds(requiredAncestor),
    });
  }
  if (requiredAncestor.id === scopeAnchor.id) break;
  requiredAncestor = requiredAncestor.parent;
}
if ("children" in scopeAnchor) {
  for (const child of scopeAnchor.children.slice(0, projectionLimit)) {
    if (!included.has(child.id)) {
      included.set(child.id, { node: child, pageChildIndex: -1, bounds: bounds(child) });
    }
  }
}
const descendantScope = scopeAnchor.type === "SECTION" || requestedPage;
let sectionTraversalTruncated = descendantScope
  && "children" in scopeAnchor
  && scopeAnchor.children.length > projectionLimit;
if (descendantScope && "children" in scopeAnchor) {
  const traversalLimit = projectionLimit * 8;
  const nestedQueue = scopeAnchor.children
    .filter((node) => !isScreenLike(node))
    .map((node) => ({ node, ancestors: [] }));
  let visited = 0;
  while (nestedQueue.length && visited < traversalLimit) {
    const { node, ancestors } = nestedQueue.shift();
    visited += 1;
    if (isScreenLike(node)) {
      const missingChain = [...ancestors, node]
        .filter((candidate) => !included.has(candidate.id));
      if (included.size + missingChain.length > projectionLimit + 2) {
        sectionTraversalTruncated = true;
        continue;
      }
      for (const candidate of missingChain) {
        included.set(candidate.id, {
          node: candidate,
          pageChildIndex: -1,
          bounds: bounds(candidate),
        });
      }
      continue;
    }
    if ("children" in node) {
      for (const child of node.children) {
        if (requestedPage && ["FRAME", "COMPONENT", "INSTANCE", "COMPONENT_SET"].includes(node.type)) continue;
        nestedQueue.push({ node: child, ancestors: [...ancestors, node] });
      }
    }
  }
  sectionTraversalTruncated ||= nestedQueue.length > 0;
}

const compact = [...included.values()]
  .filter((entry) => entry.bounds)
  .slice(0, projectionLimit + 2)
  .map(({ node, pageChildIndex, bounds: box }) => ({
    id: node.id,
    type: node.type,
    fields: {
      name: compactName(node.name),
      parentId: node.parent && node.parent.type !== "DOCUMENT" ? node.parent.id : null,
      childrenIds: [],
      x: box.x,
      y: box.y,
      width: box.width,
      height: box.height,
      childCount: "children" in node ? node.children.length : 0,
      // Select the structural projection before spending space on previews.
      textPreview: "",
      ...(requestedPage ? {} : navigationSummary(node)),
      pageChildIndex: pageChildIndex >= 0 ? pageChildIndex : null,
      // A page or the document itself has no `visible`, and Figma throws on
      // reading a property a node does not have rather than returning
      // undefined — so exploring from a page id failed outright.
      visible: !("visible" in node) || node.visible !== false,
      breadcrumb: breadcrumb(node),
    },
    extra: {},
    fieldErrors: {},
  }));

const projectedBounds = compact.reduce((result, node) => {
  const box = node.fields;
  if (!result) return { x: box.x, y: box.y, right: box.x + box.width, bottom: box.y + box.height };
  return {
    x: Math.min(result.x, box.x),
    y: Math.min(result.y, box.y),
    right: Math.max(result.right, box.x + box.width),
    bottom: Math.max(result.bottom, box.y + box.height),
  };
}, null) || { x: 0, y: 0, right: 0, bottom: 0 };
const projectionTruncated = (scopeAnchor.type !== "SECTION" && eligible.length > selected.length)
  || included.size > projectionLimit + 2
  || sectionTraversalTruncated;
const pageNode = {
  id: page.id,
  type: page.type,
  fields: {
    name: compactName(page.name),
    parentId: null,
    childrenIds: [],
    x: projectedBounds.x,
    y: projectedBounds.y,
    width: projectedBounds.right - projectedBounds.x,
    height: projectedBounds.bottom - projectedBounds.y,
    childCount: pageChildren.length,
    textPreview: "",
    projectionTruncated,
    visible: true,
    breadcrumb: breadcrumb(page),
    pageChildIndex: null,
  },
  extra: {},
  fieldErrors: {},
};

const output = {
  fileKey: figma.fileKey || "",
  version: null,
  rootIds: [page.id],
  nodes: [pageNode, ...compact.filter((node) => node.id !== page.id)],
  diagnostics: [],
};
const requiredIds = new Set([
  page.id,
  pagePeer.id,
  scopeAnchor.id,
  requestedAnchor.id,
]);
while (JSON.stringify(output).length > MAX_PROJECTION_JSON_CHARS) {
  let removableIndex = output.nodes.length - 1;
  while (removableIndex >= 0 && requiredIds.has(output.nodes[removableIndex].id)) {
    removableIndex -= 1;
  }
  if (removableIndex < 0) break;
  output.nodes.splice(removableIndex, 1);
  pageNode.fields.projectionTruncated = true;
}

if (JSON.stringify(output).length > MAX_PROJECTION_JSON_CHARS) {
  output.nodes = output.nodes
    .filter((node) => requiredIds.has(node.id))
    .map((node) => ({
      ...node,
      fields: {
        ...node.fields,
        name: compactName(node.fields.name).slice(0, 80),
        textPreview: "",
        breadcrumb: node.fields.breadcrumb.slice(-4).map((name) => name.slice(0, 80)),
        projectionTruncated: node.id === page.id ? true : node.fields.projectionTruncated,
      },
    }));
}
if (JSON.stringify(output).length > MAX_PROJECTION_JSON_CHARS) {
  throw new Error("DEVUP_EXPLORE_PROJECTION_TOO_LARGE");
}

// Optional previews may only consume space left by the structural projection.
// Page fallback can collect names across the page; bound UTF-8 bytes as well
// as the existing character budget on the metered text transport.
function utf8Bytes(value) {
  let count = 0;
  for (const character of value) {
    const point = character.codePointAt(0);
    count += point < 0x80 ? 1 : point < 0x800 ? 2 : point < 0x10000 ? 3 : 4;
  }
  return count;
}
const PAGE_TEXT_BYTES = 19 * 1024;
if (requestedPage) {
  while (utf8Bytes(JSON.stringify(output)) > PAGE_TEXT_BYTES && output.nodes.length > 1) {
    output.nodes.pop();
    pageNode.fields.projectionTruncated = true;
  }
  if (utf8Bytes(JSON.stringify(output)) > PAGE_TEXT_BYTES) throw new Error("DEVUP_EXPLORE_PROJECTION_TOO_LARGE");
}
// Count JSON escaping as well, so quotes/control characters cannot push a
// preview over the envelope cap or evict a candidate.
let previewBudget = MAX_PROJECTION_JSON_CHARS - JSON.stringify(output).length;
if (requestedPage) previewBudget = Math.min(previewBudget, PAGE_TEXT_BYTES - utf8Bytes(JSON.stringify(output)));
if (textPreviewLimit > 0 && previewBudget > 0) {
  // Each candidate takes an equal share of what is left rather than as much
  // as it wants, so the last one is funded before the first is indulged.
  // Spending the budget first-come left the tail with nothing at all, and a
  // candidate with no text is one the caller cannot tell from its
  // neighbours — which is the only thing a preview is for. What a candidate
  // does not spend stays in the budget and widens the shares after it.
  const fundable = output.nodes.filter(
    (projected) => included.get(projected.id) && projected.id !== page.id,
  );
  let unfunded = fundable.length;
  for (const projected of fundable) {
    const share = Math.floor(previewBudget / unfunded);
    unfunded -= 1;
    if (share <= 0) continue;
    let preview = "";
    let spent = 0;
    for (const character of textPreview(included.get(projected.id).node)) {
      const cost = (requestedPage ? utf8Bytes(JSON.stringify(character)) : JSON.stringify(character).length) - 2;
      if (spent + cost > share) break;
      preview += character;
      spent += cost;
    }
    projected.fields.textPreview = preview;
    previewBudget -= spent;
  }
}

return output;

}

/** source: fast_snapshot.js */
async function fastSnapshot(p) {
// Figma's own MCP runs scripts with hidden layers inside instances left out
// of `children`, `findAll` and `getNodeByIdAsync`; a plugin sees them. Set
// here so the direct path and the Devup Bridge read the same document.
figma.skipInvisibleInstanceChildren = false;
const requestedRootIds = p.rootIds;
if (!Array.isArray(requestedRootIds) || requestedRootIds.length === 0) {
  throw new Error("DEVUP_ROOTS_INVALID");
}
const roots = await Promise.all(requestedRootIds.map((id) => figma.getNodeByIdAsync(id)));
if (roots.some((root) => !root)) throw new Error("DEVUP_NODE_NOT_FOUND");
if (roots.length === 1 && roots[0].type === "SECTION") {
  throw new Error("DEVUP_TARGET_IS_SECTION");
}

// A screen drawn at three widths is three sibling frames in a Section, named
// for the width they are. Converting one of them alone can only describe that
// width, and the caller wanted the screen — so when the target is one of those
// frames, its siblings come along and the conversion can say how the screen
// changes rather than how it looks at one size.
//
// Narrow on purpose: the target must itself be named for a breakpoint, and only
// siblings that are. A Section is also how a file of unrelated cases is grouped,
// and pulling every neighbour in there would collect a catalogue to convert one
// square.
//
// The family keeps the Section's own order. It is not decoration: where one
// width does not draw a node, the plugin gives that width a hidden copy of the
// node from the first width that does, first in this order, and every value of
// the copy shows in the responsive array. Sorting the family by name here put
// tablet's values where the reference has desktop's.
const BREAKPOINT_NAMES = ["mobile", "tablet", "desktop"];
const breakpointRank = (node) => {
  const name = String(node.name || "").trim().toLowerCase();
  const known = BREAKPOINT_NAMES.indexOf(name);
  if (known >= 0) return known;
  const range = name.match(/^(\d+)\s*[~–]\s*(\d+)$/);
  if (!range) return -1;
  const start = Number(range[1]), end = Number(range[2]);
  return Number.isSafeInteger(start) && Number.isSafeInteger(end) && start > 0 && end >= start && end <= 0xffffffff ? 1 : -1;
};
if (roots.length === 1 && breakpointRank(roots[0]) >= 0) {
  const parent = roots[0].parent;
  if (parent && parent.type === "SECTION" && "children" in parent) {
    const family = parent.children
      .filter((child) => child.id === roots[0].id || breakpointRank(child) >= 0)
      .filter((child) => child.visible !== false);
    if (family.length > 1) {
      roots.length = 0;
      roots.push(...family);
    }
  }
}

// A timed Smart Animate is a chain of frames: the target, after a timeout,
// becomes its destination, which after its own timeout becomes the next, and
// so on, often back to the start. The plugin reads those frames as it goes,
// with `getNodeByIdAsync`, and turns what changes between them into CSS
// keyframes. Those frames are top-level siblings the target's subtree does
// not contain, so they are gathered here as extra roots — with the same
// narrowness as the family above: only for a single requested root, so a
// multi-root request keeps its exact root list. Every node in the subtree is
// looked at, not only the root, because the animated thing is usually inside
// the screen; and every chain member's own chain is followed, until a frame
// repeats.
if (requestedRootIds.length === 1) {
  const chainRootIds = [];
  const seen = new Set(roots.map((root) => root.id));
  const pending = [];
  const collectDestinations = (node) => {
    if (!("reactions" in node) || !Array.isArray(node.reactions)) return;
    for (const reaction of node.reactions) {
      if (!reaction || !reaction.trigger || reaction.trigger.type !== "AFTER_TIMEOUT") continue;
      for (const action of reaction.actions || []) {
        if (
          action &&
          action.type === "NODE" &&
          action.transition &&
          action.transition.type === "SMART_ANIMATE" &&
          typeof action.destinationId === "string" &&
          !seen.has(action.destinationId)
        ) {
          seen.add(action.destinationId);
          pending.push(action.destinationId);
        }
      }
    }
    if ("children" in node) for (const child of node.children) collectDestinations(child);
  };
  for (const root of roots) collectDestinations(root);
  while (pending.length > 0) {
    const id = pending.shift();
    const node = await figma.getNodeByIdAsync(id);
    if (!node || node.type === "DOCUMENT" || node.type === "PAGE") continue;
    chainRootIds.push(node);
    collectDestinations(node);
  }
  roots.push(...chainRootIds);
}
const envelopeRootId = p.nodeId;

const manifest = [
  "absoluteBoundingBox", "absoluteRenderBounds", "arcData", "backgroundStyleId", "blendMode", "bottomLeftRadius",
  "bottomRightRadius", "boundVariables", "characters", "clipsContent", "componentProperties", "componentPropertyDefinitions",
  "componentPropertyReferences", "constraints", "cornerRadius", "counterAxisAlignItems", "dashPattern", "defaultVariant",
  "effectStyleId", "effects", "fillStyleId", "fills", "fontName", "fontSize",
  "gridColumnAnchorIndex", "gridColumnCount", "gridColumnGap", "gridColumnSizes", "gridColumnSpan", "gridRowAnchorIndex",
  "gridRowCount", "gridRowGap", "gridRowSizes", "gridRowSpan", "gridStyleId", "height",
  "inferredAutoLayout", "isAsset", "isMask", "itemSpacing", "layoutAlign", "layoutGrow", "layoutMode", "layoutWrap",
  "layoutPositioning", "layoutSizingHorizontal", "layoutSizingVertical", "letterSpacing", "lineHeight", "maxHeight",
  "maxLines", "maxWidth", "minHeight", "minWidth", "name", "opacity", "overflowDirection", "overrides",
  "paddingBottom", "paddingLeft", "paddingRight", "paddingTop", "primaryAxisAlignItems", "reactions",
  "rotation", "strokeAlign", "strokeBottomWeight", "strokeLeftWeight", "strokeRightWeight", "strokeStyleId",
  "strokeTopWeight", "strokeWeight", "strokes", "targetAspectRatio", "textAlignHorizontal", "textAlignVertical",
  "textAutoResize", "textCase", "textDecoration", "textStyleId", "textTruncation", "topLeftRadius",
  "topRightRadius", "variantProperties", "visible", "width", "x", "y"
]
;
const textSegmentManifest = [
  "fontName",
  "fontWeight",
  "fontSize",
  "textDecoration",
  "textCase",
  "lineHeight",
  "letterSpacing",
  "fills",
  "textStyleId",
  "fillStyleId",
  "listOptions",
  "indentation",
  "hyperlink"
]
;
const pageOptions = p.snapshot;
const offset = Math.max(0, Math.floor(Number(pageOptions.offset) || 0));
// Upper bound for one round's node payload, before the resources block and
// the envelope around it. The ceiling that matters is on the whole text
// result: the Figma MCP cuts one at 20,480 UTF-8 bytes and appends
// "// truncated to 20kb". Measured on the official server, 2026-09-07: a
// 20,480-character ASCII string came back whole and a 20,703-character one
// was cut at 20,480; a Hangul-heavy value of 12,003 characters and 24,003
// bytes was cut at byte 20,480, mid-character, so the count is bytes and not
// characters; and 5,816 quote characters in a 20,357-character JSON array all
// survived, so JSON escaping is not counted either. The script measures
// exactly what is cut - `utf8ByteLength(JSON.stringify(envelope))` - and
// `integrity.utf8Bytes` matched the text that arrived on every one of 488
// banked pages, so a 1 KiB margin under the cut is enough. There is no PNG
// fallback any more; a page has to survive as text.
// 위 측정은 공식 MCP 가 텍스트 결과를 자르는 지점에 관한 것이다. 로컬 소켓으로
// 읽을 때는 그 자름이 없어 한 페이지에 전부 담을 수 있고, 그러면 이 스크립트를
// 수십 번 다시 부르지 않아도 된다. 천장을 옵션으로 열어 두되, 값이 없으면 위에서
// 잰 그대로 동작한다.
const envelopeCeiling = Math.max(
  8 * 1024,
  Math.floor(Number(pageOptions.maxEnvelopeBytes) || 19 * 1024),
);
const payloadCeiling = pageOptions.maxEnvelopeBytes
  ? envelopeCeiling - 1024
  : 18000;
const maxPayloadBytes = Math.min(
  payloadCeiling,
  Math.max(4096, Math.floor(Number(pageOptions.maxPayloadBytes) || 15000)),
);
const MAX_TEXT_ENVELOPE_BYTES = envelopeCeiling;

// A field whose value equals its default carries no information the converter
// can't recover from the key being absent, so it is dropped from the envelope.
// Which fields qualify is NOT a judgement call: it is proven for every rule
// below by `devup-mcp-devup-ui/tests/default_omission_golden.rs`, which
// replays this exact omission over ten real screens (1,500+ nodes) and
// requires the generated TSX to stay byte-identical. Keep the two tables in
// sync with that test.

// Figma reports an unbound style as `""`, and both readers of these fields
// already treat `""` and "absent" the same: `resources.rs::is_resource_id`
// rejects empty IDs, and `codegen/text.rs` looks the ID up in a token map
// where an empty key can never match.
const STYLE_ID_FIELDS = new Set([
  "backgroundStyleId",
  "effectStyleId",
  "fillStyleId",
  "gridStyleId",
  "strokeStyleId",
  "textStyleId",
]);

// `codegen/layout.rs` compares `view.value("maxWidth") != Some(&Value::Null)`,
// so for these two a present-null and an absent key take opposite branches.
// Their null must survive.
const NULL_SENSITIVE_FIELDS = new Set(["maxWidth", "maxHeight", "absoluteRenderBounds"]);

// Deliberately absent from this table, each because the converter branches on
// the field's *presence* rather than its value: `opacity` (hover-variant
// detection), `visible` (component registration snapshot), `layoutPositioning`
// (compared against "AUTO"), and the per-corner radii / per-side stroke
// weights (read as a group by the shorthand builders).
const SCALAR_DEFAULTS = new Map([
  ["rotation", 0],
  ["cornerRadius", 0],
  ["isAsset", false],
  ["isMask", false],
  ["clipsContent", false],
  ["blendMode", "PASS_THROUGH"],
  ["strokeAlign", "INSIDE"],
  ["textCase", "ORIGINAL"],
  ["textDecoration", "NONE"],
  ["textAlignHorizontal", "LEFT"],
  ["textAlignVertical", "TOP"],
  ["counterAxisAlignItems", "MIN"],
  ["primaryAxisAlignItems", "MIN"],
  ["gridColumnCount", 0],
  ["gridRowCount", 0],
  ["gridColumnGap", 0],
  ["gridRowGap", 0],
  ["gridColumnAnchorIndex", -1],
  ["gridRowAnchorIndex", -1],
  // A grid child spans one track unless it says otherwise.
  ["gridColumnSpan", 1],
  ["gridRowSpan", 1],
]);

// Keys a styled text segment carries that the TEXT node itself does not, so
// they must survive even when the node has a single segment.
const SEGMENT_ONLY_KEYS = new Set([
  "start",
  "end",
  "characters",
  "fontWeight",
  "textStyleId",
  "fillStyleId",
  "listOptions",
  "indentation",
  "hyperlink",
]);

function isOmittableDefault(value, name) {
  if (value === null) return !NULL_SENSITIVE_FIELDS.has(name);
  if (Array.isArray(value)) return value.length === 0;
  if (typeof value === "object") return Object.keys(value).length === 0;
  if (value === "" && STYLE_ID_FIELDS.has(name)) return true;
  return SCALAR_DEFAULTS.has(name) && SCALAR_DEFAULTS.get(name) === value;
}

// One serializer for both node fields and variable/style resources. Resources
// need the prototype chain walked (their data lives on accessors, not own
// keys) and a few structural keys skipped; node fields never do, because the
// manifest already names every property worth reading.
const RESOURCE_SKIPPED_KEYS = new Set(["parent", "children", "consumers"]);
function serialize(value, resource = false, seen = new WeakSet(), depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) return value;
  if (typeof value === "undefined") return { $undefined: true };
  if (typeof value === "bigint") return { $bigint: value.toString() };
  if (["function", "symbol"].includes(typeof value)) return { $unsupported: typeof value };
  if (depth > 12) return { $truncated: "max-depth" };
  if (
    typeof value === "object" &&
    "parent" in value &&
    typeof value.id === "string" &&
    typeof value.type === "string"
  ) {
    return { $nodeId: value.id, $nodeType: value.type };
  }
  if (Array.isArray(value)) return value.map((item) => serialize(item, resource, seen, depth + 1));
  if (ArrayBuffer.isView(value)) {
    return { $binary: value.constructor.name, byteLength: value.byteLength };
  }
  if (value instanceof ArrayBuffer) return { $binary: "ArrayBuffer", byteLength: value.byteLength };
  if (seen.has(value)) return { $circular: true };
  seen.add(value);

  let keys;
  if (resource) {
    const names = new Set(Object.keys(value));
    let current = value;
    while (current && current !== Object.prototype) {
      for (const name of Object.getOwnPropertyNames(current)) names.add(name);
      current = Object.getPrototypeOf(current);
    }
    keys = [...names].sort().filter((name) => !name.startsWith("_") && !RESOURCE_SKIPPED_KEYS.has(name));
  } else {
    keys = Object.keys(value).sort();
  }

  const result = {};
  for (const key of keys) {
    try {
      const serialized = serialize(value[key], resource, seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) result[key] = serialized;
    } catch (error) {
      result[key] = resource
        ? { $error: "unavailable" }
        : { $error: String(error && error.message ? error.message : error) };
    }
  }
  seen.delete(value);
  return result;
}

// Main-component identity is asynchronous under dynamic-page access. It is
// not a synchronous getter from the property manifest. Cache per collection
// so repacking the same page does not repeat its reads.
const devupComponentIdentity = new Map();
async function devupReadComponentIdentity(node, fields, fieldErrors) {
  if (node.type !== "INSTANCE" || typeof node.getMainComponentAsync !== "function") return;
  if (!devupComponentIdentity.has(node.id)) {
    devupComponentIdentity.set(node.id, (async () => {
      try {
        const main = await node.getMainComponentAsync();
        return { fields: {
          mainComponentId: main ? main.id : null,
          mainComponentName: main ? main.name : null,
          mainComponentSetId: main && main.parent && main.parent.type === "COMPONENT_SET" ? main.parent.id : null,
        } };
      } catch (error) {
        return { error: String(error && error.message ? error.message : error) };
      }
    })());
  }
  const identity = await devupComponentIdentity.get(node.id);
  if (identity.error) fieldErrors.mainComponentId = identity.error;
  else Object.assign(fields, identity.fields);
}
;
async function snapshotNode(node) {
  const fields = {};
  const fieldErrors = {};
  if (node.parent) fields.parentId = node.parent.id;
  // Only a root needs this. Its parent lies outside the collected subtree, so
  // the id alone says nothing, and the parent's type is what decides whether
  // the root's width is a real constraint or merely the canvas the design was
  // drawn on. Every other node's parent is collected and can be read directly,
  // so recording it there would be repetition — and repeated across a whole
  // screen it was enough to push the payload into chunked delivery.
  // Only a frame sitting directly on a page, section or component set needs
  // this: its parent is outside the collected subtree, so the id alone says
  // nothing, and the type is what decides whether its width is a real
  // constraint or the canvas it was drawn on. Keyed on the parent's type
  // rather than on being a requested root, because a multi-root collection is
  // split into batches with different root sets — the same node would then
  // carry the field in one batch and not another, and merging rejects a node
  // that arrives two different ways.
  if (
    node.parent &&
    (node.parent.type === "PAGE" ||
      node.parent.type === "SECTION" ||
      node.parent.type === "COMPONENT_SET")
  ) {
    fields.parentType = node.parent.type;
    // A screen's Section names the page component the plugin writes for it,
    // `AboutPage` for a Section called `about`, and the Section itself is
    // outside the collected subtree.
    if (node.parent.type === "SECTION") fields.parentName = node.parent.name;
  }
  const childrenIds = "children" in node ? node.children.map((child) => child.id) : [];
  if (childrenIds.length > 0) fields.childrenIds = childrenIds;

  // Only ever look at the checked-in manifest. No prototype-chain walk, no
  // "extra" bucket: an unlisted Figma Plugin API property is never collected.
  for (const name of manifest) {
    let value;
    // A variant's component properties are defined on its set, and Figma
    // refuses to read them from the variant.
    if (
      name === "componentPropertyDefinitions" &&
      node.type === "COMPONENT" &&
      node.parent &&
      node.parent.type === "COMPONENT_SET"
    ) {
      continue;
    }
    try {
      if (!(name in node)) {
        if (name === "overflowDirection" && ["FRAME", "COMPONENT", "INSTANCE", "COMPONENT_SET"].includes(node.type)) fields[name] = null; // Read attempted; absent in source.
        continue;
      }
      value = node[name];
      if (typeof value === "function") continue;
      const serialized = serialize(value);
      if (name === "overflowDirection" && value == null) { fields[name] = null; continue; }
      if (!isOmittableDefault(serialized, name)) fields[name] = serialized;
    } catch (error) {
      fieldErrors[name] = String(error && error.message ? error.message : error);
    }
  }
  if (node.type === "TEXT" && typeof node.getStyledTextSegments === "function") {
    try {
      const segments = serialize(node.getStyledTextSegments(textSegmentManifest));
      // A single segment restates typography the node already carries at the
      // top level, and `codegen/text.rs` reads the node field first and only
      // falls back to the segment. Keep just the keys that exist nowhere else.
      // Proven over 269 real single-segment text nodes by
      // `devup-mcp-devup-ui/tests/default_omission_golden.rs`.
      if (segments.length === 1) {
        const only = segments[0];
        for (const key of Object.keys(only)) {
          if (!SEGMENT_ONLY_KEYS.has(key)) delete only[key];
        }
      }
      if (segments.length > 0) fields.styledTextSegments = segments;
    } catch (error) {
      fieldErrors.styledTextSegments = String(error && error.message ? error.message : error);
    }
  }
  await devupReadComponentIdentity(node, fields, fieldErrors);
  // `extra` and `fieldErrors` are `#[serde(default)]` on the Rust `RawNode`,
  // so an empty one is the same as an absent one on the wire.
  const snapshotted = { id: node.id, type: node.type, fields };
  if (Object.keys(fieldErrors).length > 0) snapshotted.fieldErrors = fieldErrors;
  return snapshotted;
}

const allNodes = [];
const queue = [...roots];
const visitedNodeIds = new Set();
for (let index = 0; index < queue.length; index += 1) {
  const node = queue[index];
  if (visitedNodeIds.has(node.id)) continue;
  visitedNodeIds.add(node.id);
  allNodes.push(node);
  if ("children" in node) queue.push(...node.children);
}
if (offset >= allNodes.length && allNodes.length > 0) {
  throw new Error("DEVUP_SNAPSHOT_RANGE_INVALID");
}

function utf8ByteLength(value) {
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x80) bytes += 1;
    else if (code < 0x800) bytes += 2;
    else if (code >= 0xd800 && code <= 0xdbff && index + 1 < value.length) {
      bytes += 4;
      index += 1;
    } else bytes += 3;
  }
  return bytes;
}

function jsonByteLength(value) {
  return utf8ByteLength(JSON.stringify(value));
}

function styleTypeForField(field) {
  if (field === "textStyleId") return "TEXT";
  if (["fillStyleId", "strokeStyleId", "backgroundStyleId"].includes(field)) return "PAINT";
  if (field === "effectStyleId") return "EFFECT";
  if (field === "gridStyleId") return "GRID";
  return null;
}

function scanResources(value, variableIds, styleTypes) {
  if (Array.isArray(value)) {
    for (const child of value) scanResources(child, variableIds, styleTypes);
    return;
  }
  if (!value || typeof value !== "object") return;
  if (
    value.type === "VARIABLE_ALIAS" &&
    typeof value.id === "string" &&
    value.id &&
    value.id !== "figma.mixed" &&
    value.id !== "MIXED"
  ) {
    variableIds.add(value.id);
  }
  for (const [field, child] of Object.entries(value)) {
    const styleType = styleTypeForField(field);
    if (
      styleType &&
      typeof child === "string" &&
      child &&
      child !== "figma.mixed" &&
      child !== "MIXED"
    ) {
      if (!styleTypes.has(child)) styleTypes.set(child, styleType);
    }
    scanResources(child, variableIds, styleTypes);
  }
}

function aliasIdsOf(variable) {
  return Object.values(variable.valuesByMode || {})
    .filter((value) => value && value.type === "VARIABLE_ALIAS" && typeof value.id === "string" && value.id)
    .map((value) => value.id);
}

// Resolves every variable/style the given page of nodes references. Only the
// nodes shipped in THIS page are scanned, so a page's resource block stays
// consistent with its own integrity counters; devup-mcp merges across pages.
async function collectResources(nodes) {
  const variableIds = new Set();
  const styleTypes = new Map();
  scanResources(nodes, variableIds, styleTypes);

  const sortedVariableIds = [...variableIds].sort();
  const sortedStyles = [...styleTypes.entries()]
    .map(([id, styleType]) => ({ id, styleType }))
    .sort((left, right) => left.id.localeCompare(right.id));

  async function lookupVariable(id) {
    try {
      const variable = await figma.variables.getVariableByIdAsync(id);
      return variable
        ? {
            kind: "variable",
            value: serialize(variable, true),
            collectionId: variable.variableCollectionId,
            aliasIds: aliasIdsOf(variable),
          }
        : { kind: "unresolved", value: { id, kind: "variable", reason: "notFoundOrUnavailable" } };
    } catch (error) {
      return { kind: "unresolved", value: { id, kind: "variable", reason: "notFoundOrUnavailable", error: String((error && error.message) || error).slice(0, 300) } };
    }
  }
  const results = await Promise.all([
    ...sortedVariableIds.map(lookupVariable),
    ...sortedStyles.map(async ({ id, styleType }) => {
      try {
        const style = await figma.getStyleByIdAsync(id);
        if (!style) {
          return { kind: "unresolved", value: { id, kind: "style", reason: "notFoundOrUnavailable" } };
        }
        return {
          kind: "style",
          value: {
            ...serialize(style, true),
            styleType,
            value: serialize(
              styleType === "PAINT"
                ? style.paints
                : styleType === "EFFECT"
                  ? style.effects
                  : styleType === "GRID"
                    ? style.layoutGrids
                    : style,
              true,
            ),
          },
        };
      } catch (error) {
        return { kind: "unresolved", value: { id, kind: "style", reason: "notFoundOrUnavailable", error: String((error && error.message) || error).slice(0, 300) } };
      }
    }),
  ]);

  // A variable can alias another that no node in this page binds - a local
  // spacing pointing at a library's primitive. The theme needs that variable
  // to give the alias a value, so it is read too, and so is what it aliases.
  // The counts above stay the page's own references.
  const requestedVariableIds = new Set(sortedVariableIds);
  let pendingAliasIds = results.flatMap((result) => result.aliasIds || []);
  while (pendingAliasIds.length > 0) {
    const batch = [...new Set(pendingAliasIds)]
      .filter((id) => !requestedVariableIds.has(id))
      .sort();
    for (const id of batch) requestedVariableIds.add(id);
    const fetched = await Promise.all(batch.map(lookupVariable));
    results.push(...fetched);
    pendingAliasIds = fetched.flatMap((result) => result.aliasIds || []);
  }

  const collectionIds = [...new Set(results
    .filter((result) => result.kind === "variable" && result.collectionId)
    .map((result) => result.collectionId))].sort();
  const collections = (await Promise.all(collectionIds.map(async (id) => {
    try {
      const collection = await figma.variables.getVariableCollectionByIdAsync(id);
      return collection ? serialize(collection, true) : null;
    } catch (_) {
      return null;
    }
  }))).filter((collection) => collection !== null);

  const variables = results.filter((result) => result.kind === "variable").map((result) => result.value);
  const styles = results.filter((result) => result.kind === "style").map((result) => result.value);
  const unresolved = results.filter((result) => result.kind === "unresolved").map((result) => result.value);

  return {
    collections,
    variables,
    styles,
    usedRemoteVariables: variables.filter((variable) => variable.remote === true),
    usedVariableIds: sortedVariableIds,
    usedStyleIds: sortedStyles.map((style) => style.id),
    localComplete: false,
    usedRemoteComplete: unresolved.length === 0,
    unresolved,
    $variableRefCount: sortedVariableIds.length,
    $styleRefCount: sortedStyles.length,
  };
}

// Packs as many nodes as fit under `budget`, starting at `offset`. Same
// dynamic, byte-budget-driven pagination the legacy cursor snapshot uses.
async function packPage(budget) {
  const pageNodes = [];
  let payloadBytes = 2;
  for (let index = offset; index < allNodes.length; index += 1) {
    const snapshotted = await snapshotNode(allNodes[index]);
    const nodeBytes = jsonByteLength(snapshotted) + (pageNodes.length ? 1 : 0);
    if (pageNodes.length && payloadBytes + nodeBytes > budget) break;
    pageNodes.push(snapshotted);
    payloadBytes += nodeBytes;
  }
  return { pageNodes, packedBytes: payloadBytes };
}

function buildEnvelope(pageNodes, resources) {
  const nextOffset = Math.min(allNodes.length, offset + pageNodes.length);
  const { $variableRefCount, $styleRefCount, ...resourceBlock } = resources;
  const nodes = [
    ...pageNodes,
    {
      id: "__DEVUP_SNAPSHOT_CURSOR__",
      type: "DEVUP_INTERNAL",
      // `offset` is what lets the Rust decoder tell a first page from a
      // continuation page, which decides whether the root must be present
      // here. All four fields are read by the shared `read_snapshot_cursor`.
      fields: {
        offset,
        nextOffset,
        complete: nextOffset >= allNodes.length,
        totalNodes: allNodes.length,
      },
      extra: {},
      fieldErrors: {},
    },
  ];
  const envelope = {
    kind: "devupFastSnapshotEnvelope",
    schemaVersion: 1,
    source: { fileKey: figma.fileKey || "", rootId: envelopeRootId },
    snapshot: {
      fileKey: figma.fileKey || "",
      version: null,
      rootIds: roots.map((root) => root.id),
      nodes,
      diagnostics: [],
    },
    resources: resourceBlock,
    // No `pagination` mirror: the __DEVUP_SNAPSHOT_CURSOR__ marker node is the
    // single source of truth for page state, and duplicating it is exactly how
    // the two copies drifted apart before.
    integrity: {
      nodeCount: nodes.length,
      variableRefCount: $variableRefCount,
      styleRefCount: $styleRefCount,
      utf8Bytes: 0,
    },
  };
  // Writing the byte count into the envelope changes the envelope's own
  // length, so iterate to the fixed point. `utf8ByteLength` measures without
  // building a throwaway byte array.
  let bytes = 0;
  for (let attempt = 0; attempt < 8; attempt += 1) {
    bytes = utf8ByteLength(JSON.stringify(envelope));
    if (envelope.integrity.utf8Bytes === bytes) break;
    envelope.integrity.utf8Bytes = bytes;
  }
  if (envelope.integrity.utf8Bytes !== utf8ByteLength(JSON.stringify(envelope))) {
    throw new Error("DEVUP_ENVELOPE_LENGTH_UNSTABLE");
  }
  return { envelope, bytes };
}

// The node budget alone can't bound the envelope: a page also carries every
// variable/style its nodes reference, and that block is only sized once the
// nodes are chosen. So pack, build, and if the whole envelope overshoots the
// text limit, take the overshoot off the node budget and try again. Fewer
// nodes can only reference fewer resources, so the next envelope is at least
// the overshoot smaller and lands under the limit. Halving the budget
// instead, as this did, gave up half a page over a few hundred bytes: of 340
// banked pages, 143 ended where the next node would still have fit.
let nodeBudget = maxPayloadBytes - 1024;
let built = null;
for (let attempt = 0; attempt < 5; attempt += 1) {
  const { pageNodes, packedBytes } = await packPage(nodeBudget);
  if (pageNodes.length === 0) throw new Error("DEVUP_SNAPSHOT_RANGE_INVALID");
  const candidate = buildEnvelope(pageNodes, await collectResources(pageNodes));
  if (candidate.bytes <= MAX_TEXT_ENVELOPE_BYTES) {
    built = candidate;
    break;
  }
  if (pageNodes.length === 1) {
    // A single node whose own resources blow the limit; no smaller page
    // exists and there is no binary transport to fall back to.
    throw new Error("DEVUP_ENVELOPE_TOO_LARGE");
  }
  const overshoot = candidate.bytes - MAX_TEXT_ENVELOPE_BYTES;
  nodeBudget = Math.max(1, Math.min(nodeBudget - 1, packedBytes - overshoot - 256));
}
if (!built) throw new Error("DEVUP_ENVELOPE_TOO_LARGE");
return built.envelope;

}

/** source: fast_theme.js */
async function fastTheme(p) {
// Figma's own MCP runs scripts with hidden layers inside instances left out
// of `children`, `findAll` and `getNodeByIdAsync`; a plugin sees them. Set
// here so the direct path and the Devup Bridge read the same document.
figma.skipInvisibleInstanceChildren = false;
const MAX_ENVELOPE_BYTES = 8 * 1024 * 1024;
// The Figma MCP cuts a text result at 20,480 UTF-8 bytes; the measurement is
// written up in fast_snapshot.js.
const MAX_TEXT_ENVELOPE_BYTES = 19 * 1024;
// A file's resources rarely fit one answer - forty variables with their
// modes are twice this - so they are paged: everything the theme is made of
// (the resources, and the scan of which ids are used and what could not be
// resolved) is one list in a fixed order, and a page is a run of it from
// `offset` that says where the next one starts. The theme used to be one
// envelope or nothing, and a file whose theme did not fit fell back to a
// path that knows only local styles. The scan is paged like the rest
// because it is not small: a file with three hundred used ids carries
// 20 KB of them, a whole page on its own.
const pageOptions = p.theme;
const pageOffset = Math.max(0, Math.floor(Number(pageOptions.offset) || 0));

function propertyNames(value) {
  const names = new Set(Object.keys(value));
  let current = value;
  while (current && current !== Object.prototype) {
    for (const name of Object.getOwnPropertyNames(current)) names.add(name);
    current = Object.getPrototypeOf(current);
  }
  return [...names].sort();
}

function serialize(value, seen = new WeakSet(), depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) return value;
  if (typeof value === "undefined") return { $undefined: true };
  if (typeof value === "bigint") return { $bigint: value.toString() };
  if (["function", "symbol"].includes(typeof value)) return { $unsupported: typeof value };
  if (depth > 12) return { $truncated: "max-depth" };
  if (
    typeof value === "object" &&
    "parent" in value &&
    typeof value.id === "string" &&
    typeof value.type === "string"
  ) {
    return { $nodeId: value.id, $nodeType: value.type };
  }
  if (Array.isArray(value)) return value.map((item) => serialize(item, seen, depth + 1));
  if (ArrayBuffer.isView(value)) {
    return { $binary: value.constructor.name, byteLength: value.byteLength };
  }
  if (value instanceof ArrayBuffer) return { $binary: "ArrayBuffer", byteLength: value.byteLength };
  if (seen.has(value)) return { $circular: true };
  seen.add(value);
  const output = {};
  for (const name of propertyNames(value)) {
    if (name.startsWith("_") || ["parent", "children", "consumers"].includes(name)) continue;
    try {
      const serialized = serialize(value[name], seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) output[name] = serialized;
    } catch (_) {
      output[name] = { $error: "unavailable" };
    }
  }
  seen.delete(value);
  return output;
}

function styleTypeForField(field) {
  if (field === "textStyleId") return "TEXT";
  if (["fillStyleId", "strokeStyleId", "backgroundStyleId"].includes(field)) return "PAINT";
  if (field === "effectStyleId") return "EFFECT";
  if (field === "gridStyleId") return "GRID";
  return null;
}

const usedVariableIds = new Set();
const usedStyleTypes = new Map();
function scanResources(value, fieldName = "", seen = new WeakSet(), depth = 0) {
  if (depth > 16 || value === null || typeof value !== "object") return;
  if (seen.has(value)) return;
  seen.add(value);
  if (Array.isArray(value)) {
    for (const child of value) scanResources(child, fieldName, seen, depth + 1);
    return;
  }
  if (
    value.type === "VARIABLE_ALIAS" &&
    typeof value.id === "string" &&
    value.id &&
    value.id !== "figma.mixed" &&
    value.id !== "MIXED"
  ) {
    usedVariableIds.add(value.id);
  }
  for (const [field, child] of Object.entries(value)) {
    const styleType = styleTypeForField(field);
    if (
      styleType &&
      typeof child === "string" &&
      child &&
      child !== "figma.mixed" &&
      child !== "MIXED"
    ) {
      if (!usedStyleTypes.has(child)) usedStyleTypes.set(child, styleType);
    }
    scanResources(child, field || fieldName, seen, depth + 1);
  }
}

// Under Figma's own MCP a page is not loaded until something loads it, and
// `findAll` sees nothing on it: the devup Test file's second page answered
// 0 nodes there and 70 to the plugin, which loads every page when it starts.
for (const page of figma.root.children) await page.loadAsync();
const documentNodes = [figma.root, ...figma.root.findAll(() => true)];
for (const node of documentNodes) {
  for (const field of [
    "boundVariables",
    "fills",
    "strokes",
    "effects",
    "layoutGrids",
    "textStyleId",
    "fillStyleId",
    "strokeStyleId",
    "backgroundStyleId",
    "effectStyleId",
    "gridStyleId",
  ]) {
    try {
      scanResources({ [field]: node[field] });
    } catch (_) {}
  }
}

const [localCollections, localVariables, paints, texts, effects, grids] = await Promise.all([
  figma.variables.getLocalVariableCollectionsAsync(),
  figma.variables.getLocalVariablesAsync(),
  figma.getLocalPaintStylesAsync(),
  figma.getLocalTextStylesAsync(),
  figma.getLocalEffectStylesAsync(),
  figma.getLocalGridStylesAsync(),
]);
const localVariableIds = new Set(localVariables.map((variable) => variable.id));
const localStyleIds = new Set([...paints, ...texts, ...effects, ...grids].map((style) => style.id));

const unresolved = [];
async function remoteVariable(id) {
  try {
    const variable = await figma.variables.getVariableByIdAsync(id);
    return variable || null;
  } catch (error) {
    unresolved.push({ id, kind: "variable", reason: "notFoundOrUnavailable", error: String((error && error.message) || error).slice(0, 300) });
    return null;
  }
}
// A variable can alias a library variable no node binds - a local spacing
// that points at a library's primitive. The theme needs that variable to give
// the alias a value, so it is read too, and so is whatever it aliases.
function aliasTargets(variable) {
  return Object.values(variable.valuesByMode || {})
    .filter((value) => value && value.type === "VARIABLE_ALIAS" && typeof value.id === "string" && value.id)
    .map((value) => value.id);
}
const remoteStyleJobs = [...usedStyleTypes.entries()]
  .filter(([id]) => !localStyleIds.has(id))
  .sort(([left], [right]) => left.localeCompare(right))
  .map(async ([id, styleType]) => {
    try {
      const style = await figma.getStyleByIdAsync(id);
      return style ? { style, styleType } : null;
    } catch (error) {
      unresolved.push({ id, kind: "style", reason: "notFoundOrUnavailable", error: String((error && error.message) || error).slice(0, 300) });
      return null;
    }
  });
const requestedRemoteIds = new Set();
const remoteVariables = [];
let pendingRemoteIds = [...usedVariableIds, ...localVariables.flatMap(aliasTargets)];
while (pendingRemoteIds.length > 0) {
  const batch = [...new Set(pendingRemoteIds)]
    .filter((id) => !localVariableIds.has(id) && !requestedRemoteIds.has(id))
    .sort();
  for (const id of batch) requestedRemoteIds.add(id);
  const fetched = (await Promise.all(batch.map(remoteVariable))).filter(Boolean);
  remoteVariables.push(...fetched);
  pendingRemoteIds = fetched.flatMap(aliasTargets);
}
const remoteStyles = (await Promise.all(remoteStyleJobs)).filter(Boolean);
for (const id of requestedRemoteIds) {
  if (!remoteVariables.some((variable) => variable.id === id) && !unresolved.some((item) => item.id === id)) {
    unresolved.push({ id, kind: "variable", reason: "notFoundOrUnavailable" });
  }
}
for (const [id] of [...usedStyleTypes.entries()].filter(([id]) => !localStyleIds.has(id))) {
  if (!remoteStyles.some((item) => item.style.id === id) && !unresolved.some((item) => item.id === id)) {
    unresolved.push({ id, kind: "style", reason: "notFoundOrUnavailable" });
  }
}

const remoteCollectionIds = [...new Set(remoteVariables.map((variable) => variable.variableCollectionId))]
  .filter((id) => !localCollections.some((collection) => collection.id === id))
  .sort();
const remoteCollections = (
  await Promise.all(
    remoteCollectionIds.map(async (id) => {
      try {
        return await figma.variables.getVariableCollectionByIdAsync(id);
      } catch (_) {
        return null;
      }
    }),
  )
).filter(Boolean);

function serializeStyle(style, styleType) {
  return {
    ...serialize(style),
    styleType,
    value: serialize(
      styleType === "PAINT"
        ? style.paints
        : styleType === "EFFECT"
          ? style.effects
          : styleType === "GRID"
            ? style.layoutGrids
            : style,
    ),
  };
}

const styles = [
  ...paints.map((style) => serializeStyle(style, "PAINT")),
  ...texts.map((style) => serializeStyle(style, "TEXT")),
  ...effects.map((style) => serializeStyle(style, "EFFECT")),
  ...grids.map((style) => serializeStyle(style, "GRID")),
  ...remoteStyles.map(({ style, styleType }) => serializeStyle(style, styleType)),
].sort((left, right) => left.id.localeCompare(right.id));
const variables = [...localVariables, ...remoteVariables]
  .map((variable) => serialize(variable))
  .sort((left, right) => left.id.localeCompare(right.id));
const collections = [...localCollections, ...remoteCollections]
  .map((collection) => serialize(collection))
  .sort((left, right) => left.id.localeCompare(right.id));
unresolved.sort((left, right) => left.kind.localeCompare(right.kind) || left.id.localeCompare(right.id));

function utf8Encode(value) {
  const bytes = [];
  for (let index = 0; index < value.length; index += 1) {
    let codePoint = value.charCodeAt(index);
    if (codePoint >= 0xd800 && codePoint <= 0xdbff) {
      const next = index + 1 < value.length ? value.charCodeAt(index + 1) : 0;
      if (next >= 0xdc00 && next <= 0xdfff) {
        codePoint = 0x10000 + ((codePoint - 0xd800) << 10) + (next - 0xdc00);
        index += 1;
      } else {
        codePoint = 0xfffd;
      }
    } else if (codePoint >= 0xdc00 && codePoint <= 0xdfff) {
      codePoint = 0xfffd;
    }
    if (codePoint < 0x80) bytes.push(codePoint);
    else if (codePoint < 0x800) bytes.push(0xc0 | (codePoint >> 6), 0x80 | (codePoint & 0x3f));
    else if (codePoint < 0x10000) {
      bytes.push(0xe0 | (codePoint >> 12), 0x80 | ((codePoint >> 6) & 0x3f), 0x80 | (codePoint & 0x3f));
    } else {
      bytes.push(
        0xf0 | (codePoint >> 18),
        0x80 | ((codePoint >> 12) & 0x3f),
        0x80 | ((codePoint >> 6) & 0x3f),
        0x80 | (codePoint & 0x3f),
      );
    }
  }
  return new Uint8Array(bytes);
}

// Everything in one order: collections, variables, styles, then the scan -
// the used variable ids, the used style ids, the unresolved. A page is a run
// of this list.
const items = [
  ...collections.map((value) => ({ kind: "collection", value })),
  ...variables.map((value) => ({ kind: "variable", value })),
  ...styles.map((value) => ({ kind: "style", value })),
  ...[...usedVariableIds].sort().map((value) => ({ kind: "usedVariableId", value })),
  ...[...usedStyleTypes.keys()].sort().map((value) => ({ kind: "usedStyleId", value })),
  ...unresolved.map((value) => ({ kind: "unresolved", value })),
];
if (pageOffset > items.length) throw new Error("DEVUP_SNAPSHOT_RANGE_INVALID");

function buildEnvelope(pageItems, nextOffset) {
  const of = (kind) => pageItems.filter((item) => item.kind === kind).map((item) => item.value);
  const pageCollections = of("collection");
  const pageVariables = of("variable");
  const pageStyles = of("style");
  const pageUnresolved = of("unresolved");
  const envelope = {
    kind: "devupFastThemeEnvelope",
    schemaVersion: 1,
    source: { fileKey: figma.fileKey || "", version: null },
    resources: {
      collections: pageCollections,
      variables: pageVariables,
      styles: pageStyles,
      // Derived by the collector from `variables` once the pages are merged;
      // listing a page's remote variables twice halved what a page held.
      usedRemoteVariables: [],
      usedVariableIds: of("usedVariableId"),
      usedStyleIds: of("usedStyleId"),
      localComplete: true,
      usedRemoteComplete: unresolved.length === 0,
      unresolved: pageUnresolved,
    },
    // Read by the Rust decoder: a page is one of several when `complete` is
    // false, and the next one is asked for from `nextOffset`.
    page: {
      offset: pageOffset,
      nextOffset,
      complete: nextOffset >= items.length,
      totalItems: items.length,
    },
    integrity: {
      collectionCount: pageCollections.length,
      variableCount: pageVariables.length,
      styleCount: pageStyles.length,
      unresolvedCount: pageUnresolved.length,
      utf8Bytes: 0,
    },
  };
  let envelopeBytes = new Uint8Array();
  for (let attempt = 0; attempt < 8; attempt += 1) {
    envelopeBytes = utf8Encode(JSON.stringify(envelope));
    if (envelope.integrity.utf8Bytes === envelopeBytes.length) break;
    envelope.integrity.utf8Bytes = envelopeBytes.length;
  }
  envelopeBytes = utf8Encode(JSON.stringify(envelope));
  if (envelope.integrity.utf8Bytes !== envelopeBytes.length) {
    throw new Error("DEVUP_ENVELOPE_LENGTH_UNSTABLE");
  }
  return { envelope, bytes: envelopeBytes.length };
}

// Pack from the offset by the items' own sizes until the budget is spent,
// then build the page; the envelope around the items - and the remote
// variables a page lists twice - is only sized once built, so if the whole
// overshoots, take the overshoot off the budget and pack again. Fewer items
// can only make a smaller envelope, so this converges. One item alone that
// does not fit cannot be split, and is reported rather than cut.
const itemBytes = items.map((item) => utf8Encode(JSON.stringify(item.value)).length + 1);
function packFrom(budget) {
  const pageItems = [];
  let packed = 0;
  for (let index = pageOffset; index < items.length; index += 1) {
    if (pageItems.length > 0 && packed + itemBytes[index] > budget) break;
    pageItems.push(items[index]);
    packed += itemBytes[index];
  }
  return { pageItems, packed };
}
let budget = MAX_TEXT_ENVELOPE_BYTES - 1024;
let built = null;
for (let attempt = 0; attempt < 6; attempt += 1) {
  const { pageItems, packed } = packFrom(budget);
  const candidate = buildEnvelope(pageItems, pageOffset + pageItems.length);
  if (candidate.bytes <= MAX_TEXT_ENVELOPE_BYTES) {
    built = candidate;
    break;
  }
  if (pageItems.length <= 1) break;
  budget = Math.max(1, Math.min(budget - 1, packed - (candidate.bytes - MAX_TEXT_ENVELOPE_BYTES) - 256));
}
if (built === null) throw new Error("DEVUP_ENVELOPE_TOO_LARGE");
if (built.bytes > MAX_ENVELOPE_BYTES) throw new Error("DEVUP_ENVELOPE_TOO_LARGE");
return built.envelope;

}

/** source: large_value.js */
async function largeValue(p) {
// Figma's own MCP runs scripts with hidden layers inside instances left out
// of `children`, `findAll` and `getNodeByIdAsync`; a plugin sees them. Set
// here so the direct path and the Devup Bridge read the same document.
figma.skipInvisibleInstanceChildren = false;
const options = p.largeValue;
// Read-only recovery. Never clone, detach, crop a parent or manufacture a
// substitute: the returned geometry must belong to this exact subtree.
async function devupLookupAssetNode(id) {
  try {
    const node = await figma.getNodeByIdAsync(id);
    if (node) return node;
  } catch (error) {
    if (!id.startsWith("I") || !id.includes(";")) throw error;
    try { return await walk(); }
    catch (fallback) { throw new Error(`${error.message || error}; nested lookup fallback: ${fallback.message || fallback}`); }
  }
  if (!id.startsWith("I") || !id.includes(";")) return null;
  return walk();

  async function walk() {
    const root = await figma.getNodeByIdAsync(id.slice(1).split(";")[0]);
    const pending = root ? [root] : [];
    const seen = new Set();
    while (pending.length) {
      const node = pending.pop();
      if (node.id === id) return node;
      if (seen.has(node.id)) continue;
      seen.add(node.id);
      if (seen.size > 10000) throw new Error("nested lookup exceeds 10000 nodes");
      if (Array.isArray(node.children)) pending.push(...node.children);
    }
    throw new Error(`nested node ${id} not found in its instance`);
  }
}

function devupGeometrySvg(root) {
  const escape = value => String(value).replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&apos;"}[c]));
  const finite = value => typeof value === "number" && Number.isFinite(value);
  const fail = (node, reason) => { throw new Error(`geometry fallback unsupported: ${node.type || "unknown"} ${node.id || ""}: ${reason}`); };
  let geometryCount = 0, visited = 0;
  function paintPaths(node, paths, paints) {
    if ((paints !== undefined && !Array.isArray(paints)) || (paints === undefined && Array.isArray(paths) && paths.length)) fail(node, "unresolved paint requires native export");
    const visible = Array.isArray(paints) ? paints.filter(p => p.visible !== false) : [];
    if (!visible.length) return "";
    if (!Array.isArray(paths) || !paths.length) fail(node, "paint has no rendered geometry");
    return visible.map(paint => {
      if (paint.type !== "SOLID" || (paint.blendMode && paint.blendMode !== "NORMAL")) fail(node, "paint type or blending requires native export");
      const color = paint.color;
      if (!color || ![color.r, color.g, color.b].every(c => finite(c) && c >= 0 && c <= 1)) fail(node, "invalid solid color");
      const opacity = paint.opacity === undefined ? 1 : paint.opacity;
      if (!finite(opacity) || opacity < 0 || opacity > 1) fail(node, "invalid paint opacity");
      // Paths of one winding rule are painted together, preserving holes and
      // overlap within the rendered geometry. Preserve floating-point colors.
      return ["NONZERO", "EVENODD"].map(rule => {
        const matching = paths.filter(path => path.windingRule === rule);
        if (!matching.length) return "";
        for (const path of matching) {
          if (typeof path.data !== "string" || !path.data.trim()) fail(node, "empty path");
          geometryCount++;
        }
        return `<path d="${escape(matching.map(p => p.data).join(" "))}" fill="rgb(${color.r * 100}%,${color.g * 100}%,${color.b * 100}%)" fill-opacity="${opacity}" fill-rule="${rule === "EVENODD" ? "evenodd" : "nonzero"}"/>`;
      }).join("");
    }).join("");
  }
  function render(node, isRoot) {
    if (++visited > 10000) fail(node, "subtree exceeds 10000 nodes");
    if (node.isMask) fail(node, "mask requires native export");
    if (node.visible === false || node.opacity === 0) return "";
    if (node.clipsContent) fail(node, "clipping requires native export");
    if (Array.isArray(node.effects) && node.effects.some(e => e.visible !== false)) fail(node, "effect requires native export");
    if (node.blendMode && !["NORMAL", "PASS_THROUGH"].includes(node.blendMode)) fail(node, "layer blending requires native export");
    const opacity = node.opacity === undefined ? 1 : node.opacity;
    if (!finite(opacity) || opacity < 0 || opacity > 1) fail(node, "invalid layer opacity");
    let transform = "";
    if (!isRoot) {
      const t = node.relativeTransform;
      if (!Array.isArray(t) || t.length !== 2 || !t.every(row => Array.isArray(row) && row.length === 3 && row.every(finite))) fail(node, "missing relative transform");
      transform = ` transform="matrix(${t[0][0]} ${t[1][0]} ${t[0][1]} ${t[1][1]} ${t[0][2]} ${t[1][2]})"`;
    }
    let body = "";
    if (["GROUP", "FRAME", "INSTANCE", "COMPONENT"].includes(node.type)) {
      if ((Array.isArray(node.fills) && node.fills.some(p => p.visible !== false)) || (Array.isArray(node.strokes) && node.strokes.some(p => p.visible !== false))) fail(node, "container paint requires native export");
      if (!Array.isArray(node.children)) fail(node, "missing children");
      const children = node.itemReverseZIndex === true ? [...node.children].reverse() : node.children;
      body = children.map(child => render(child, false)).join("");
    } else {
      if (!["VECTOR", "BOOLEAN_OPERATION", "RECTANGLE", "ELLIPSE", "POLYGON", "STAR", "LINE"].includes(node.type)) fail(node, "node type requires native export");
      // Plugin API strokeGeometry is centered even for INSIDE/OUTSIDE strokes.
      // Using it there would silently change the shape.
      if (Array.isArray(node.strokes) && node.strokes.some(p => p.visible !== false) && node.strokeAlign !== "CENTER") fail(node, "non-center stroke alignment requires native export");
      if (Array.isArray(node.dashPattern) && node.dashPattern.length) fail(node, "dashed stroke requires native export");
      for (const paths of [node.fillGeometry, node.strokeGeometry]) {
        if (paths !== undefined && (!Array.isArray(paths) || paths.some(p => !["NONZERO", "EVENODD"].includes(p.windingRule)))) fail(node, "unknown winding rule");
      }
      // Baked geometry includes boolean operations and centered stroke outlines.
      // Never approximate those using a bounding box.
      body = paintPaths(node, node.fillGeometry, node.fills) + paintPaths(node, node.strokeGeometry, node.strokes);
    }
    return `<g opacity="${opacity}"${transform}>${body}</g>`;
  }
  const bounds = root.absoluteRenderBounds, t = root.absoluteTransform;
  // Native exports include stroke extents. Convert rendered bounds back to
  // local space only when this is exact; rotated/sheared bounds lose that info.
  if (!bounds || ![bounds.x, bounds.y, bounds.width, bounds.height].every(finite) || bounds.width <= 0 || bounds.height <= 0) fail(root, "missing render bounds");
  if (!Array.isArray(t) || t.length !== 2 || !t.every(row => Array.isArray(row) && row.length === 3 && row.every(finite)) || t[0][1] !== 0 || t[1][0] !== 0 || t[0][0] <= 0 || t[1][1] <= 0) fail(root, "root rotation, reflection or shear requires native export");
  const x = (bounds.x - t[0][2]) / t[0][0], y = (bounds.y - t[1][2]) / t[1][1];
  const width = bounds.width / t[0][0], height = bounds.height / t[1][1];
  const body = render(root, true);
  if (!geometryCount) fail(root, "no painted geometry");
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="${x} ${y} ${width} ${height}">${body}</svg>`;
}

async function devupRenderAsset(node, settings, nativeBudgetMs = 85000, observe = () => {}) {
  const began = Date.now();
  let timer;
  try {
    const result = await Promise.race([
      node.exportAsync(settings),
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`DEVUP_ASSET_TIMEOUT: native export exceeded ${nativeBudgetMs} ms`)), nativeBudgetMs); }),
    ]);
    observe("native-export", Date.now() - began);
    return result;
  }
  catch (nativeError) {
    observe("native-export", Date.now() - began, String(nativeError.message || nativeError).slice(0, 500));
    if (settings.format !== "SVG_STRING") throw nativeError;
    const fallbackBegan = Date.now();
    try {
      const svg = devupGeometrySvg(node);
      observe("geometry-fallback", Date.now() - fallbackBegan);
      return svg;
    }
    catch (fallbackError) { throw new Error(`native export: ${nativeError.message || nativeError}; ${fallbackError.message || fallbackError}`); }
  } finally { clearTimeout(timer); }
}
;
const node = await devupLookupAssetNode(options.nodeId);
if (!node) throw new Error("DEVUP_NODE_NOT_FOUND");

const textSegmentManifest = [
  "fontName",
  "fontWeight",
  "fontSize",
  "textDecoration",
  "textCase",
  "lineHeight",
  "letterSpacing",
  "fills",
  "textStyleId",
  "fillStyleId",
  "listOptions",
  "indentation",
  "hyperlink"
]
;

function serialize(value, seen = new WeakSet(), depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) return value;
  if (typeof value === "undefined") return { $undefined: true };
  if (typeof value === "bigint") return { $bigint: value.toString() };
  if (["function", "symbol"].includes(typeof value)) return { $unsupported: typeof value };
  if (depth > 12) return { $truncated: "max-depth" };
  if (
    typeof value === "object" &&
    "parent" in value &&
    typeof value.id === "string" &&
    typeof value.type === "string"
  ) {
    return { $nodeId: value.id, $nodeType: value.type };
  }
  if (Array.isArray(value)) return value.map((item) => serialize(item, seen, depth + 1));
  if (ArrayBuffer.isView(value)) {
    return { $binary: value.constructor.name, byteLength: value.byteLength };
  }
  if (value instanceof ArrayBuffer) return { $binary: "ArrayBuffer", byteLength: value.byteLength };
  if (seen.has(value)) return { $circular: true };
  seen.add(value);
  const result = {};
  for (const key of Object.keys(value).sort()) {
    try {
      const serialized = serialize(value[key], seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) result[key] = serialized;
    } catch (error) {
      result[key] = { $error: String(error && error.message ? error.message : error) };
    }
  }
  seen.delete(value);
  return result;
}

function utf8Encode(value) {
  const bytes = [];
  for (let index = 0; index < value.length; index += 1) {
    let codePoint = value.charCodeAt(index);
    if (codePoint >= 0xd800 && codePoint <= 0xdbff) {
      const next = index + 1 < value.length ? value.charCodeAt(index + 1) : 0;
      if (next >= 0xdc00 && next <= 0xdfff) {
        codePoint = 0x10000 + ((codePoint - 0xd800) << 10) + (next - 0xdc00);
        index += 1;
      } else codePoint = 0xfffd;
    } else if (codePoint >= 0xdc00 && codePoint <= 0xdfff) codePoint = 0xfffd;
    if (codePoint < 0x80) bytes.push(codePoint);
    else if (codePoint < 0x800) {
      bytes.push(0xc0 | (codePoint >> 6), 0x80 | (codePoint & 0x3f));
    } else if (codePoint < 0x10000) {
      bytes.push(
        0xe0 | (codePoint >> 12),
        0x80 | ((codePoint >> 6) & 0x3f),
        0x80 | (codePoint & 0x3f),
      );
    } else {
      bytes.push(
        0xf0 | (codePoint >> 18),
        0x80 | ((codePoint >> 12) & 0x3f),
        0x80 | ((codePoint >> 6) & 0x3f),
        0x80 | (codePoint & 0x3f),
      );
    }
  }
  return new Uint8Array(bytes);
}

function sha256(bytes) {
  const constants = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
    0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
    0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
  ];
  const length = bytes.length;
  const paddedLength = Math.ceil((length + 9) / 64) * 64;
  const padded = new Uint8Array(paddedLength);
  padded.set(bytes);
  padded[length] = 0x80;
  const bitLength = length * 8;
  for (let index = 0; index < 8; index += 1) {
    padded[paddedLength - 1 - index] = Math.floor(bitLength / 2 ** (index * 8)) & 0xff;
  }
  const hash = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
    0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ];
  const rotate = (value, bits) => (value >>> bits) | (value << (32 - bits));
  for (let offset = 0; offset < padded.length; offset += 64) {
    const words = new Uint32Array(64);
    for (let index = 0; index < 16; index += 1) {
      const start = offset + index * 4;
      words[index] =
        (padded[start] << 24) |
        (padded[start + 1] << 16) |
        (padded[start + 2] << 8) |
        padded[start + 3];
    }
    for (let index = 16; index < 64; index += 1) {
      const s0 = rotate(words[index - 15], 7) ^ rotate(words[index - 15], 18) ^ (words[index - 15] >>> 3);
      const s1 = rotate(words[index - 2], 17) ^ rotate(words[index - 2], 19) ^ (words[index - 2] >>> 10);
      words[index] = (words[index - 16] + s0 + words[index - 7] + s1) >>> 0;
    }
    let [a, b, c, d, e, f, g, h] = hash;
    for (let index = 0; index < 64; index += 1) {
      const s1 = rotate(e, 6) ^ rotate(e, 11) ^ rotate(e, 25);
      const choice = (e & f) ^ (~e & g);
      const temp1 = (h + s1 + choice + constants[index] + words[index]) >>> 0;
      const s0 = rotate(a, 2) ^ rotate(a, 13) ^ rotate(a, 22);
      const majority = (a & b) ^ (a & c) ^ (b & c);
      const temp2 = (s0 + majority) >>> 0;
      h = g; g = f; f = e; e = (d + temp1) >>> 0;
      d = c; c = b; b = a; a = (temp1 + temp2) >>> 0;
    }
    for (const [index, value] of [a, b, c, d, e, f, g, h].entries()) {
      hash[index] = (hash[index] + value) >>> 0;
    }
  }
  return hash.map((value) => value.toString(16).padStart(8, "0")).join("");
}

function base64(bytes) {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let output = "";
  for (let index = 0; index < bytes.length; index += 3) {
    const first = bytes[index];
    const second = index + 1 < bytes.length ? bytes[index + 1] : 0;
    const third = index + 2 < bytes.length ? bytes[index + 2] : 0;
    output += alphabet[first >> 2];
    output += alphabet[((first & 3) << 4) | (second >> 4)];
    output += index + 1 < bytes.length ? alphabet[((second & 15) << 2) | (third >> 6)] : "=";
    output += index + 2 < bytes.length ? alphabet[third & 63] : "=";
  }
  return output;
}

let rawValue;
// The bytes a fragment is cut from. A node field is its JSON; the virtual
// field `$export:svg` is the node's SVG export as text, which the asset
// script announced when it was too large to carry in one answer.
let exportedBytes = null;
try {
  if (options.field === "$export:svg") {
    if (typeof node.exportAsync !== "function") throw new Error("unsupported");
    const svg = await devupRenderAsset(node, { format: "SVG_STRING" }, Math.max(1, (Number(options.timeoutMs) || 90000) - 5000));
    if (typeof svg !== "string") throw new Error("unsupported");
    exportedBytes = utf8Encode(svg);
  } else if (typeof options.field === "string" && options.field.startsWith("$export:png")) {
    // A PNG too large for one attachment, re-exported a fragment at a time.
    // The scale rides on the field - `$export:png@2` - so the re-export is
    // the same bytes the asset script announced, hash for hash.
    if (typeof node.exportAsync !== "function") throw new Error("unsupported");
    const scale = Number(options.field.split("@")[1] || 2);
    if (!Number.isFinite(scale) || scale <= 0 || scale > 4) throw new Error("invalid scale");
    const parts = options.field.split("@");
    let capture = node;
    if (parts[2] === "visible") {
      const seen = new Set();
      while (capture && capture.id !== parts[3] && !seen.has(capture) && seen.size < 64) { seen.add(capture); capture = capture.parent; }
      if (!capture || capture.id !== parts[3] || ["PAGE", "DOCUMENT"].includes(capture.type)) throw new Error("invalid visible scope");
    } else if (parts.length > 2) throw new Error("invalid export field");
    const png = await capture.exportAsync({ format: "PNG", contentsOnly: true, useAbsoluteBounds: false, constraint: { type: "SCALE", value: scale } });
    exportedBytes = png instanceof Uint8Array ? png : new Uint8Array(png);
  } else if (
    options.field === "styledTextSegments" &&
    node.type === "TEXT" &&
    typeof node.getStyledTextSegments === "function"
  ) {
    rawValue = node.getStyledTextSegments(textSegmentManifest);
  } else if (options.field in node) {
    rawValue = node[options.field];
  } else {
    throw new Error("unsupported");
  }
} catch (error) {
  return {
    kind: "devupLargeValueUnsupported",
    fileKey: figma.fileKey || "",
    version: options.version,
    nodeId: options.nodeId,
    field: options.field,
    byteLength: options.byteLength,
    sha256: options.sha256,
    errorCode: "DEVUP_FIELD_UNSUPPORTED_BY_UPSTREAM",
    errorDetail: `${options.field} failed: ${error.message || error}`.slice(0, 500),
  };
}

const bytes = exportedBytes === null ? utf8Encode(JSON.stringify(serialize(rawValue))) : exportedBytes;
const observedHash = sha256(bytes);
if (bytes.length !== options.byteLength || observedHash !== options.sha256) {
  throw new Error("DEVUP_LARGE_VALUE_CHANGED");
}
const offset = Math.max(0, Math.floor(Number(options.offset) || 0));
const maxChunkBytes = Math.min(65536, Math.max(1, Math.floor(Number(options.maxChunkBytes) || 8192)));
if (offset >= bytes.length) throw new Error("DEVUP_LARGE_VALUE_RANGE_INVALID");
const nextOffset = Math.min(bytes.length, offset + maxChunkBytes);
return {
  kind: "devupLargeValueFragment",
  fileKey: figma.fileKey || "",
  version: options.version,
  nodeId: options.nodeId,
  field: options.field,
  offset,
  nextOffset,
  byteLength: bytes.length,
  sha256: observedHash,
  dataBase64: base64(bytes.slice(offset, nextOffset)),
  complete: nextOffset === bytes.length,
};

}

/** source: measure_nodes.js */
async function measureNodes(p) {
// A fixed read of explicit IDs. Never walk children or acquire resources.
const ids = p.rootIds;
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

}

/** source: metadata.js */
async function metadata(p) {
// Figma's own MCP runs scripts with hidden layers inside instances left out
// of `children`, `findAll` and `getNodeByIdAsync`; a plugin sees them. Set
// here so the direct path and the Devup Bridge read the same document.
figma.skipInvisibleInstanceChildren = false;
// `get_metadata` 가 주던 노드 계층을 플러그인 API 로 그대로 만든다.
//
// 공식 경로는 XML 을 주고 이 경로는 JSON 을 주지만, 받는 쪽은 한 곳이다.
// `metadata.rs` 의 `find_metadata` 가 첫 분기에서 `MetadataDocument` 를 그대로
// 역직렬화하고, XML 은 그 다음 분기에서 같은 구조체로 환원된다. 그래서 여기서
// 내보낼 것은 그 구조체의 camelCase 표현 하나뿐이다 — 필드를 더 얹으면 두 경로가
// 갈라진다.
//
//   { fileKey, version, rootId, nodes: [{ id, type, name, childrenIds, descendantCount }] }
//
// 좌표와 크기는 넣지 않는다. 디코더가 읽지 않고, 레이아웃은 snapshot 이 맡는다.

const root = await figma.getNodeByIdAsync(p.nodeId);
if (!root) throw new Error("DEVUP_NODE_NOT_FOUND");

// 다이나믹 페이지 문서에서는 자식이 아직 메모리에 없을 수 있다. search.js 와 같은
// 방법으로 대상이 속한 페이지를 먼저 올린다.
let page = root;
while (page && page.type !== "PAGE") page = page.parent;
if (page && page.type === "PAGE") await figma.setCurrentPageAsync(page);

const nodes = [];

// 자손 수를 돌려주면서 목록을 채운다. 깊이가 아주 깊은 문서에서 호출 스택이
// 터지지 않도록 명시적 스택으로 후위 순회한다.
function collect(start) {
  const stack = [{ node: start, visited: false }];
  const counts = new Map();
  while (stack.length > 0) {
    const frame = stack[stack.length - 1];
    const node = frame.node;
    const children = "children" in node ? node.children : [];
    if (!frame.visited) {
      frame.visited = true;
      for (let index = children.length - 1; index >= 0; index -= 1) {
        stack.push({ node: children[index], visited: false });
      }
      continue;
    }
    stack.pop();
    let descendantCount = 0;
    const childrenIds = [];
    for (const child of children) {
      childrenIds.push(child.id);
      descendantCount += 1 + (counts.get(child.id) || 0);
    }
    counts.set(node.id, descendantCount);
    nodes.push({
      id: node.id,
      type: node.type,
      name: node.name,
      childrenIds,
      descendantCount,
    });
  }
}

collect(root);

return {
  fileKey: figma.fileKey || "",
  version: null,
  rootId: root.id,
  nodes,
};

}

/** source: page_catalog.js */
async function pageCatalog(p) {
const pages = figma.root.children;
return {
  fileKey: figma.fileKey || "",
  version: null,
  rootIds: pages.map((page) => page.id),
  nodes: pages.map((page) => ({
    id: page.id,
    type: page.type,
    fields: {
      name: page.name,
      parentId: null,
      childrenIds: [],
    },
    extra: {},
    fieldErrors: {},
  })),
  diagnostics: [],
};

}

/** source: reference_png.js */
async function referencePng(p) {
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
const nodeId = p.nodeId;
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
    if (!["LAYER_BLUR", "DROP_SHADOW", "INNER_SHADOW", "BACKGROUND_BLUR"].includes(effect.type)) {
      approximate.push(`${owner} has a ${effect.type}`);
    }
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
  // Others go as they are, so their pixels stay what they were. The UI decodes
  // one image at a time and all of a scene's images share one allowance of
  // time, so stalled conversions still leave time to send the canvas picture.
  // A conversion is counted before it is asked for, every buffer it makes
  // added up and none taken to be freed early: the image's bytes here, the
  // message carrying them, the copy the UI receives, the Blob made from it
  // and what the decoder reads; the decoded image, and a resampled copy when
  // it is drawn smaller; the canvas; the PNG encoded from it (at most 5 bytes
  // a pixel, each row's filter byte and deflate's overhead included), the
  // ArrayBuffer read from it, the message back and the copy received here;
  // and the encoder's working memory. An image over 512 MiB that way, or one
  // left when the time is up, goes as it is. Images are counted as they are
  // read: past 64 MiB together the rest are not encoded, as the answer will
  // be the canvas picture.
  const normalize = globalThis.__devupNormalizeImage;
  const normalizeUntil = Date.now() + 20000;
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
      const fit = Math.min(1, 8192 / Math.max(width, height, 1));
      const drawn = Math.ceil(width * fit) * Math.ceil(height * fit);
      const encoded = 5 * drawn + 64 * 1024;
      const peak = 5 * bytes.length + 4 * width * height * (fit < 1 ? 2 : 1) + 4 * drawn + 4 * encoded
        + 1024 * 1024;
      const left = normalizeUntil - Date.now();
      if ((!decoded || Math.max(width, height) > 8192 || bytes.length > 4 * width * height + 1024 * 1024)
        && peak <= 512 * 1024 * 1024 && left > 0) {
        const png = await normalize(bytes, 8192, left, width, height);
        if (png) {
          bytes = png;
          laidOut = { width, height };
        }
      }
    }
    if (imageBytes + bytes.length > 64 * 1024 * 1024) {
      approximate.push(`its ancestors' images are over ${64 * 1024 * 1024} bytes`);
      break;
    }
    imageBytes += bytes.length;
    imageData[hash] = laidOut ? { data: figma.base64Encode(bytes), ...laidOut } : figma.base64Encode(bytes);
  }
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

}

/** source: reference_png_chunk.js */
async function referencePngChunk(p) {
// The next piece of a referencePng answer that referencePng held because it
// did not fit one bridge answer. Bridge only: the direct path never asks.
const options = p.largeValue;
const store = globalThis.__devupReferencePngAnswers;
const entry = store && store.get(options.id);
if (!entry || typeof entry.text !== "string") {
  throw new Error(`DEVUP_REFERENCE_PNG_ANSWER_GONE ${options.id}`);
}
const text = entry.text;
const offset = Math.max(0, Math.floor(Number(options.offset) || 0));
const end = Math.min(text.length, offset + 4 * 1024 * 1024);
if (end >= text.length) store.delete(options.id);
return { kind: "devupReferencePngChunk", id: options.id, offset, nextOffset: end, complete: end >= text.length, text: text.slice(offset, end) };

}

/** source: search.js */
async function search(p) {
// Figma's own MCP runs scripts with hidden layers inside instances left out
// of `children`, `findAll` and `getNodeByIdAsync`; a plugin sees them. Set
// here so the direct path and the Devup Bridge read the same document.
figma.skipInvisibleInstanceChildren = false;
const root = await figma.getNodeByIdAsync(p.nodeId);
if (!root) throw new Error("DEVUP_NODE_NOT_FOUND");
let page = root;
while (page && page.type !== "PAGE") page = page.parent;
if (!page || page.type !== "PAGE") throw new Error("DEVUP_PAGE_NOT_FOUND");
await page.loadAsync();

const options = p.search;
const defaultTypes = ["PAGE", "SECTION", "FRAME", "COMPONENT_SET", "COMPONENT"];
const allowedTypes = new Set(
  (options.nodeTypes.length ? options.nodeTypes : defaultTypes).map((value) => value.toUpperCase()),
);

function normalize(value) {
  return value.normalize("NFC").toLocaleLowerCase().replace(/\s/gu, "");
}

function levenshtein(left, right) {
  let previous = Array.from({ length: right.length + 1 }, (_, index) => index);
  for (let leftIndex = 0; leftIndex < left.length; leftIndex += 1) {
    const current = [leftIndex + 1];
    for (let rightIndex = 0; rightIndex < right.length; rightIndex += 1) {
      current.push(
        Math.min(
          current[rightIndex] + 1,
          previous[rightIndex + 1] + 1,
          previous[rightIndex] + (left[leftIndex] === right[rightIndex] ? 0 : 1),
        ),
      );
    }
    previous = current;
  }
  return previous[right.length];
}

function score(name) {
  const trimmed = options.query.trim();
  if (trimmed === "" || trimmed === "*") return 100;
  if (name === options.query) return 400;
  if (options.matchKind === "exact") return null;
  const candidate = normalize(name);
  const query = normalize(options.query);
  if (candidate === query) return 300;
  if (candidate.startsWith(query)) return 200;
  if (candidate.includes(query)) return 100;
  if (options.matchKind !== "fuzzy") return null;
  const distance = levenshtein(Array.from(candidate), Array.from(query));
  const threshold = Math.min(4, Math.max(1, Math.ceil(Array.from(query).length / 4)));
  return distance <= threshold ? Math.max(0, 50 - distance) : null;
}

function excluded(node) {
  for (let current = node; current && current.type !== "DOCUMENT"; current = current.parent) {
    // SECTION nodes have no opacity; reading it throws in the plugin API.
    if (current.visible === false || ("opacity" in current && current.opacity === 0)) return true;
    const name = (current.name || "").toLowerCase();
    if (["trash", "archive", "draft", "copy", "hidden", "휴지통", "보관", "초안", "복사", "pdf", "저장용", "export", "출력"].some(keyword => name.includes(keyword))) return true;
  }
  return false;
}

const descendants = root === page
  ? page.findAll(() => true)
  : "findAll" in root ? root.findAll(() => true) : [];
const ranked = [root, ...descendants]
  .filter((node) => allowedTypes.has(node.type) && typeof node.name === "string")
  .map((node) => ({ node, score: score(node.name) }))
  .filter((entry) => entry.score !== null)
  .sort((left, right) =>
    Number(excluded(left.node)) - Number(excluded(right.node)) ||
    right.score - left.score ||
    left.node.name.localeCompare(right.node.name) ||
    left.node.id.localeCompare(right.node.id),
  );
const candidates = ranked.slice(0, options.limit);

const included = new Map([[page.id, page]]);
// Keep the anchor and its ancestry even when nothing matches. Consumers
// need the chain to resolve scope and distinguish empty from missing roots.
for (const node of [root, ...candidates.map((entry) => entry.node)]) {
  let current = node;
  while (current && current.type !== "DOCUMENT") {
    included.set(current.id, current);
    current = current.parent;
  }
}

return {
  fileKey: figma.fileKey || "",
  version: null,
  rootIds: [page.id],
  nodes: [...included.values()].map((node) => ({
    id: node.id,
    type: node.type,
    fields: {
      name: node.name,
      visible: node.visible !== false,
      opacity: "opacity" in node && typeof node.opacity === "number" ? node.opacity : 1,
      searchCandidatesTruncated: node === root && ranked.length > candidates.length,
      parentId: node.parent && node.parent.type !== "DOCUMENT" ? node.parent.id : null,
      childrenIds: "children" in node ? node.children.map((child) => child.id) : [],
    },
    extra: {},
    fieldErrors: {},
  })),
  diagnostics: [],
};

}

/** source: section_index.js */
async function sectionIndex(p) {
// Carry a small measured navigation summary into screen planning, before a
// full page export. Missing measurements remain unknown rather than guessed.
function navigationContentWidth(root) {
  const queue = [root];
  let visited = 0;
  let width = null;
  while (queue.length && visited++ < 400) {
    const node = queue.shift();
    if (("visible" in node) && node.visible === false) continue;
    const children = "children" in node ? node.children.filter((child) => !("visible" in child) || child.visible !== false) : [];
    if (/header|nav|gnb/i.test(node.name || "") && children.length >= 2
      && ("layoutMode" in node) && node.layoutMode === "HORIZONTAL") {
      const sizes = children.map(bounds);
      if (sizes.every(Boolean)) {
        const measured = sizes.reduce((sum, box) => sum + box.width, 0) + (Number(node.itemSpacing) || 0) * (sizes.length - 1);
        width = Math.max(width || 0, measured);
      }
    }
    queue.push(...children.slice(0, 80));
  }
  return width;
}

function navigationSummary(root) {
  const width = navigationContentWidth(root);
  return width === null ? {} : { navigationContentWidth: width };
}


// Figma's own MCP runs scripts with hidden layers inside instances left out
// of `children`, `findAll` and `getNodeByIdAsync`; a plugin sees them. Set
// here so the direct path and the Devup Bridge read the same document.
figma.skipInvisibleInstanceChildren = false;
const section = await figma.getNodeByIdAsync(p.nodeId);
if (!section) throw new Error("DEVUP_NODE_NOT_FOUND");
if (section.type !== "SECTION") {
  let ancestor = section.parent;
  const seen = new Set();
  while (ancestor && ancestor.type !== "SECTION" && !seen.has(ancestor.id)) {
    seen.add(ancestor.id);
    ancestor = ancestor.parent;
  }
  const sectionId = ancestor && ancestor.type === "SECTION" ? ancestor.id : null;
  const url = sectionId && figma.fileKey
    ? `https://www.figma.com/design/${figma.fileKey}?node-id=${sectionId.replace(/:/g, "-")}` : null;
  throw new Error("DEVUP_SECTION_REQUIRED " + JSON.stringify({
    pluginCode: "DEVUP_SECTION_REQUIRED", stage: "section-index",
    nodeId: section.id, nodeType: section.type, sectionId,
    nextAction: {
      tool: "devup_figma_export",
      how: sectionId ? `The url must point to a SECTION. This capture's SECTION is ${sectionId}; select frames with frameIds.`
        : "The url must point to a SECTION. This node has no ancestor SECTION; choose the intended SECTION in Figma and copy its link.",
      arguments: url ? (section.type === "FRAME" && section.parent === ancestor
        ? {url, frameIds: [section.id]} : {url}) : null,
      requiredArguments: url ? [] : ["url"]
    }
  }));
}

const MAX_CANDIDATES = 100;
const MAX_TRAVERSED_NODES = 20000;
const MAX_PREVIEW_CHARACTERS = 120;
const MAX_PREVIEW_NODES = 64;
let remainingPreviewBytes = 2048;

// The menu needs enough copy to distinguish similarly named frames, without
// returning their descendants or letting previews dominate the compact index.
//
// `allowance` is this candidate's share of the budget rather than the whole
// of what is left. Reading the pool directly spent it first-come: on the
// Braillify Studio login Section the first seven candidates took their full
// 120 characters — Korean is three UTF-8 bytes each, so 360 bytes apiece
// against a 2,048-byte pool — and the remaining seven came back
// `budget-exhausted` with nothing to read. That file names four frames
// `AUTH-01` and four `Frame 3`, so a candidate without text is a candidate
// that cannot be identified, and the previews that did land were three
// strings repeated twice over. Screens differ in their opening characters,
// so a short preview for every candidate is worth more than a long one for
// the first few.
function textPreview(root, allowance) {
  const queue = [root];
  let preview = "";
  let characters = 0;
  let bytes = 0;
  let scanTruncated = false;
  if (allowance <= 0) return { text: "", state: "budget-exhausted", spent: 0 };
  for (let index = 0; index < queue.length; index += 1) {
    const node = queue[index];
    if (node.visible === false) continue;
    if (node.type === "TEXT" && typeof node.characters === "string") {
      const text = node.characters.replace(/\s+/g, " ").trim();
      for (const character of (preview && text ? " " : "") + text) {
        const size = utf8ByteLength(character);
        if (characters >= MAX_PREVIEW_CHARACTERS || bytes + size > allowance) {
          return {
            text: preview.trimEnd(),
            state: bytes + size > allowance ? "budget-exhausted" : "truncated",
            spent: bytes,
          };
        }
        preview += character;
        characters += 1;
        bytes += size;
      }
    }
    if ("children" in node) {
      const room = Math.max(0, MAX_PREVIEW_NODES - queue.length);
      if (node.children.length > room) scanTruncated = true;
      queue.push(...node.children.slice(0, room));
    }
  }
  return {
    text: preview,
    state: scanTruncated ? "truncated" : (preview ? "available" : "no-text"),
    spent: bytes,
  };
}

function bounds(node) {
  const value = node.absoluteBoundingBox || {
    x: typeof node.x === "number" ? node.x : 0,
    y: typeof node.y === "number" ? node.y : 0,
    width: typeof node.width === "number" ? node.width : 0,
    height: typeof node.height === "number" ? node.height : 0,
  };
  if (![value.x, value.y, value.width, value.height].every(Number.isFinite)) return null;
  return { x: value.x, y: value.y, width: value.width, height: value.height };
}

function isScreen(node, box) {
  if (node.type !== "FRAME" || node.visible === false || !box) return false;
  const aspect = box.width / Math.max(1, box.height);
  return box.width >= 240 && box.width <= 1800
    && box.height >= 300 && box.height <= 2000
    && aspect >= 0.25 && aspect <= 2.5;
}

function breadcrumb(node) {
  const names = [];
  let current = node;
  while (current && current.type !== "DOCUMENT") {
    if (typeof current.name === "string" && current.name) names.push(current.name);
    current = current.parent;
  }
  return names.reverse();
}

function utf8ByteLength(value) {
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x80) bytes += 1;
    else if (code < 0x800) bytes += 2;
    else if (code >= 0xd800 && code <= 0xdbff && index + 1 < value.length) {
      bytes += 4;
      index += 1;
    } else bytes += 3;
  }
  return bytes;
}

function subtreeEstimate(root) {
  const queue = [root];
  let count = 0;
  let estimatedBytes = 0;
  for (let index = 0; index < queue.length && count < MAX_TRAVERSED_NODES; index += 1) {
    const node = queue[index];
    count += 1;
    estimatedBytes += 512 + utf8ByteLength(typeof node.name === "string" ? node.name : "");
    if (node.type === "TEXT" && typeof node.characters === "string") {
      estimatedBytes += utf8ByteLength(node.characters) * 2;
    }
    if ("children" in node) queue.push(...node.children);
  }
  return {
    subtreeNodeCount: count,
    estimatedSerializedBytes: estimatedBytes,
    truncated: queue.length > count,
  };
}

const queue = "children" in section ? [...section.children] : [];
// Cache edges during the existing traversal; ownership must not cross the
// plugin bridge once per ancestor of every descendant.
const traversalParents = new Map(queue.map(node => [node.id, section.id]));
const candidateNodes = [];
let traversalCount = 0;
for (let index = 0; index < queue.length && traversalCount < MAX_TRAVERSED_NODES; index += 1) {
  const node = queue[index];
  traversalCount += 1;
  const box = bounds(node);
  if (isScreen(node, box)) {
    let parent = node.parent;
    let nestedInScreen = false;
    while (parent && parent.id !== section.id) {
      if (isScreen(parent, bounds(parent))) {
        nestedInScreen = true;
        break;
      }
      parent = parent.parent;
    }
    if (!nestedInScreen) candidateNodes.push({ node, box });
  }
  if ("children" in node) {
    const children = node.children;
    for (const child of children) traversalParents.set(child.id, node.id);
    queue.push(...children);
  }
}
// Screen shape is a guess for finding screens on a page that has no grouping.
// A Section is grouping, already explicit, and the guess applied there answers
// with whatever happens to measure like a phone. A Section of small cases
// annotated with tall notes turns it upside down: the notes pass and the cases
// do not, so the index offered the notes and hid every case — an answer that
// looked complete, which is worse than the empty list a Section of cases used
// to give. What the Section holds is what it offers.
if ("children" in section) {
  const chosen = new Set(candidateNodes.map(({ node }) => node.id));
  for (const node of section.children) {
    if (chosen.has(node.id)) continue;
    const box = bounds(node);
    if (!box || node.visible === false) continue;
    // Keep the whole child available for explicit selection alongside any
    // automatic screens inside it. Rust cannot recover discarded descendants.
    candidateNodes.push({ node, box });
  }
}
candidateNodes.sort((left, right) =>
  left.box.y - right.box.y
    || left.box.x - right.box.x
    || left.node.id.localeCompare(right.node.id),
);
const projectionTruncated = queue.length > traversalCount || candidateNodes.length > MAX_CANDIDATES;
const selected = candidateNodes.slice(0, MAX_CANDIDATES);
const selectedIds = new Set(selected.map(({ node }) => node.id));
// Only selection queries need ownership in this response. Export diagnostics
// use the collected snapshot's parentId/childrenIds on the server instead.
const requestedIds = new Set(p.rootIds);
const nodeScreenIds = {};
const owners = new Map([[section.id, null]]);
for (let index = 0; index < traversalCount; index += 1) {
  const id = queue[index].id;
  const owner = selectedIds.has(id) ? id : owners.get(traversalParents.get(id)) ?? null;
  owners.set(id, owner);
  if (requestedIds.has(id)) nodeScreenIds[id] = owner;
}
// Link to the nearest retained ancestor: intermediate layout groups are not
// included in this compact projection, but container/screen ancestry survives.
const parentIds = new Map(selected.map(({ node }) => {
  let parent = node.parent;
  while (parent && parent.id !== section.id && !selectedIds.has(parent.id)) {
    parent = parent.parent;
  }
  return [node.id, parent && selectedIds.has(parent.id) ? parent.id : section.id];
}));
const childrenIds = (id) => selected
  .filter(({ node }) => parentIds.get(node.id) === id)
  .map(({ node }) => node.id);
const sectionBox = bounds(section);
if (!sectionBox) throw new Error("DEVUP_NODE_BOUNDS_UNAVAILABLE");

const sectionNode = {
  id: section.id,
  type: section.type,
  fields: {
    name: section.name,
    parentId: section.parent && section.parent.type !== "DOCUMENT" ? section.parent.id : null,
    childrenIds: childrenIds(section.id),
    absoluteBoundingBox: sectionBox,
    visible: section.visible !== false,
    projectionTruncated,
    nodeScreenIds,
  },
  extra: {},
  fieldErrors: {},
};
let unfunded = selected.length;
const candidates = selected.map(({ node, box }) => {
  const estimate = subtreeEstimate(node);
  // An equal share of what is left, so the last candidate is funded before
  // the first is indulged. What a candidate does not spend stays in the pool
  // and widens every share after it, so a menu of short texts still reads in
  // full.
  const allowance = Math.floor(remainingPreviewBytes / unfunded);
  unfunded -= 1;
  const preview = textPreview(node, allowance);
  remainingPreviewBytes -= preview.spent;
  return {
    id: node.id,
    type: node.type,
    fields: {
      name: typeof node.name === "string" ? node.name : "",
      parentId: parentIds.get(node.id),
      childrenIds: childrenIds(node.id),
      absoluteBoundingBox: box,
      visible: node.visible !== false,
      breadcrumb: breadcrumb(node),
      directChildCount: "children" in node ? node.children.length : 0,
      textPreview: preview.text,
      ...navigationSummary(node),
      textPreviewState: preview.state,
      subtreeNodeCount: estimate.subtreeNodeCount,
      estimatedSerializedBytes: estimate.estimatedSerializedBytes,
      selectionReasons: [isScreen(node, box) ? "screen-like" : "explicit-selection-only", "inside-section"],
      estimateTruncated: estimate.truncated,
    },
    extra: {},
    fieldErrors: {},
  };
});

const result = {
  fileKey: figma.fileKey || "",
  version: null,
  rootIds: [section.id],
  nodes: [sectionNode, ...candidates],
  diagnostics: [],
};
// The measured upstream ceiling is 20,480 UTF-8 bytes. Never hand it JSON
// that it will *silently* cut; what crosses has to describe itself.
let responseBytes = utf8ByteLength(JSON.stringify(result));
// Previews are optional menu hints. Yield their space first: a hint is never
// worth a selectable screen.
for (let index = candidates.length - 1; index >= 0 && responseBytes > 19 * 1024; index -= 1) {
  candidates[index].fields.textPreview = "";
  candidates[index].fields.textPreviewState = "budget-exhausted";
  responseBytes = utf8ByteLength(JSON.stringify(result));
}
// Still over. Refusing outright returned *nothing* for a Section whose menu
// was merely long, and left no way forward: the ceiling is a property of the
// Section's size, so the same request reproduces it forever. Drop from the
// end and declare it with the same `projectionTruncated` the traversal and
// candidate caps above already raise — the server refuses `allScreens` on a
// truncated index and answers `not-found-in-truncated-index` instead of
// claiming a node is absent. A short menu that admits it is short beats an
// empty one. Silence was the thing forbidden here, not truncation.
const retained = () => result.nodes.length - 1;
const discoveredCandidates = candidates.length;
while (responseBytes > 19 * 1024 && retained() > 1) {
  const dropped = result.nodes.pop();
  sectionNode.fields.projectionTruncated = true;
  sectionNode.fields.childrenIds = sectionNode.fields.childrenIds
    .filter((id) => id !== dropped.id);
  responseBytes = utf8ByteLength(JSON.stringify(result));
}
if (responseBytes > 19 * 1024) {
  throw new Error("DEVUP_SECTION_INDEX_TOO_LARGE " + JSON.stringify({
    pluginCode: "DEVUP_SECTION_INDEX_TOO_LARGE", stage: "section-index",
    responseBytes, maxResponseBytes: 19 * 1024,
    discoveredCandidates, retainedCandidates: retained(),
  }));
}
return result;

}

/** source: snapshot.js */
async function snapshot(p) {
// Figma's own MCP runs scripts with hidden layers inside instances left out
// of `children`, `findAll` and `getNodeByIdAsync`; a plugin sees them. Set
// here so the direct path and the Devup Bridge read the same document.
figma.skipInvisibleInstanceChildren = false;
const root = await figma.getNodeByIdAsync(p.nodeId);
if (!root) throw new Error("DEVUP_NODE_NOT_FOUND");

const manifest = [
  "absoluteBoundingBox", "absoluteRenderBounds", "arcData", "backgroundStyleId", "blendMode", "bottomLeftRadius",
  "bottomRightRadius", "boundVariables", "characters", "clipsContent", "componentProperties", "componentPropertyDefinitions",
  "componentPropertyReferences", "constraints", "cornerRadius", "counterAxisAlignItems", "dashPattern", "defaultVariant",
  "effectStyleId", "effects", "fillStyleId", "fills", "fontName", "fontSize",
  "gridColumnAnchorIndex", "gridColumnCount", "gridColumnGap", "gridColumnSizes", "gridColumnSpan", "gridRowAnchorIndex",
  "gridRowCount", "gridRowGap", "gridRowSizes", "gridRowSpan", "gridStyleId", "height",
  "inferredAutoLayout", "isAsset", "isMask", "itemSpacing", "layoutAlign", "layoutGrow", "layoutMode", "layoutWrap",
  "layoutPositioning", "layoutSizingHorizontal", "layoutSizingVertical", "letterSpacing", "lineHeight", "maxHeight",
  "maxLines", "maxWidth", "minHeight", "minWidth", "name", "opacity", "overflowDirection", "overrides",
  "paddingBottom", "paddingLeft", "paddingRight", "paddingTop", "primaryAxisAlignItems", "reactions",
  "rotation", "strokeAlign", "strokeBottomWeight", "strokeLeftWeight", "strokeRightWeight", "strokeStyleId",
  "strokeTopWeight", "strokeWeight", "strokes", "targetAspectRatio", "textAlignHorizontal", "textAlignVertical",
  "textAutoResize", "textCase", "textDecoration", "textStyleId", "textTruncation", "topLeftRadius",
  "topRightRadius", "variantProperties", "visible", "width", "x", "y"
]
;
const manifestSet = new Set(manifest);
const textSegmentManifest = [
  "fontName",
  "fontWeight",
  "fontSize",
  "textDecoration",
  "textCase",
  "lineHeight",
  "letterSpacing",
  "fills",
  "textStyleId",
  "fillStyleId",
  "listOptions",
  "indentation",
  "hyperlink"
]
;
const snapshotOptions = p.snapshot;
const offset = Math.max(0, Math.floor(Number(snapshotOptions.offset) || 0));
// 이 천장들은 공식 MCP 가 텍스트 결과를 자르는 지점에서 나왔다. 로컬 소켓으로
// 읽을 때는 자름이 없으므로 한 번에 다 담을 수 있고, 그러면 큰 필드를 따로
// 받아오는 chunk-read 왕복이 통째로 사라진다. 값이 없으면 기존 그대로다.
const payloadCeiling = snapshotOptions.maxEnvelopeBytes
  ? Math.max(8 * 1024, Math.floor(Number(snapshotOptions.maxEnvelopeBytes)))
  : 16000;
const maxPayloadBytes = Math.min(
  payloadCeiling,
  Math.max(4096, Math.floor(Number(snapshotOptions.maxPayloadBytes) || 15000)),
);
const maxFieldBytes = Math.min(
  maxPayloadBytes - 1024,
  Math.max(512, Math.floor(Number(snapshotOptions.maxFieldBytes) || 4096)),
);
// `mainComponent`, `masterComponent` and `instances` are the synchronous
// getters Figma refuses under dynamic-page document access ("Use
// node.getMainComponentAsync instead"). Nothing here reads them, and walking
// the prototype chain put one refusal per instance and component into
// `fieldErrors`: 593 on the devup Test file.
const skipped = new Set(["id", "type", "parent", "children", "mainComponent", "masterComponent", "instances"]);
const hardProtectedFields = new Set([
  "parentId",
  "childrenIds",
  "name",
  "characters",
  "styledTextSegments",
  "boundVariables",
  "overrides",
]);

const DEVUP_MAX_LARGE_VALUE_BYTES = 16 * 1024 * 1024;
// A fragment's text is the chunk in base64, four thirds of it, inside a
// wrapper of some three hundred bytes, and the Figma MCP cuts a text result
// at 20,480 UTF-8 bytes (measured; see fast_snapshot.js). Twelve KiB comes
// back as 16.7 KB of text, with room to spare; eight KiB was 11.2 KB, and
// took a third more calls for the same value.
const DEVUP_LARGE_VALUE_CHUNK_BYTES = 12288;

function devupUtf8Encode(value) {
  const bytes = [];
  for (let index = 0; index < value.length; index += 1) {
    let codePoint = value.charCodeAt(index);
    if (codePoint >= 0xd800 && codePoint <= 0xdbff) {
      const next = index + 1 < value.length ? value.charCodeAt(index + 1) : 0;
      if (next >= 0xdc00 && next <= 0xdfff) {
        codePoint = 0x10000 + ((codePoint - 0xd800) << 10) + (next - 0xdc00);
        index += 1;
      } else codePoint = 0xfffd;
    } else if (codePoint >= 0xdc00 && codePoint <= 0xdfff) codePoint = 0xfffd;
    if (codePoint < 0x80) bytes.push(codePoint);
    else if (codePoint < 0x800) {
      bytes.push(0xc0 | (codePoint >> 6), 0x80 | (codePoint & 0x3f));
    } else if (codePoint < 0x10000) {
      bytes.push(
        0xe0 | (codePoint >> 12),
        0x80 | ((codePoint >> 6) & 0x3f),
        0x80 | (codePoint & 0x3f),
      );
    } else {
      bytes.push(
        0xf0 | (codePoint >> 18),
        0x80 | ((codePoint >> 12) & 0x3f),
        0x80 | ((codePoint >> 6) & 0x3f),
        0x80 | (codePoint & 0x3f),
      );
    }
  }
  return new Uint8Array(bytes);
}

function devupSha256(bytes) {
  const constants = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
    0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
    0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
  ];
  const paddedLength = Math.ceil((bytes.length + 9) / 64) * 64;
  const padded = new Uint8Array(paddedLength);
  padded.set(bytes);
  padded[bytes.length] = 0x80;
  const bitLength = bytes.length * 8;
  for (let index = 0; index < 8; index += 1) {
    padded[paddedLength - 1 - index] = Math.floor(bitLength / 2 ** (index * 8)) & 0xff;
  }
  const hash = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
    0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ];
  const rotate = (value, bits) => (value >>> bits) | (value << (32 - bits));
  for (let offset = 0; offset < padded.length; offset += 64) {
    const words = new Uint32Array(64);
    for (let index = 0; index < 16; index += 1) {
      const start = offset + index * 4;
      words[index] =
        (padded[start] << 24) |
        (padded[start + 1] << 16) |
        (padded[start + 2] << 8) |
        padded[start + 3];
    }
    for (let index = 16; index < 64; index += 1) {
      const s0 = rotate(words[index - 15], 7) ^ rotate(words[index - 15], 18) ^ (words[index - 15] >>> 3);
      const s1 = rotate(words[index - 2], 17) ^ rotate(words[index - 2], 19) ^ (words[index - 2] >>> 10);
      words[index] = (words[index - 16] + s0 + words[index - 7] + s1) >>> 0;
    }
    let [a, b, c, d, e, f, g, h] = hash;
    for (let index = 0; index < 64; index += 1) {
      const s1 = rotate(e, 6) ^ rotate(e, 11) ^ rotate(e, 25);
      const choice = (e & f) ^ (~e & g);
      const first = (h + s1 + choice + constants[index] + words[index]) >>> 0;
      const s0 = rotate(a, 2) ^ rotate(a, 13) ^ rotate(a, 22);
      const majority = (a & b) ^ (a & c) ^ (b & c);
      const second = (s0 + majority) >>> 0;
      h = g; g = f; f = e; e = (d + first) >>> 0;
      d = c; c = b; b = a; a = (first + second) >>> 0;
    }
    for (const [index, value] of [a, b, c, d, e, f, g, h].entries()) {
      hash[index] = (hash[index] + value) >>> 0;
    }
  }
  return hash.map((value) => value.toString(16).padStart(8, "0")).join("");
}

function devupLargeValueDescriptor(nodeId, field, serialized) {
  const bytes = devupUtf8Encode(JSON.stringify(serialized));
  if (bytes.length > DEVUP_MAX_LARGE_VALUE_BYTES) {
    return { $truncated: "max-large-value-bytes", byteLength: bytes.length };
  }
  return {
    $largeValue: {
      nodeId,
      field,
      byteLength: bytes.length,
      sha256: devupSha256(bytes),
      cursor: { nextOffset: 0, maxChunkBytes: DEVUP_LARGE_VALUE_CHUNK_BYTES },
    },
  };
}
;

function utf8ByteLength(value) {
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x80) bytes += 1;
    else if (code < 0x800) bytes += 2;
    else if (code >= 0xd800 && code <= 0xdbff && index + 1 < value.length) {
      bytes += 4;
      index += 1;
    } else bytes += 3;
  }
  return bytes;
}

function jsonByteLength(value) {
  return utf8ByteLength(JSON.stringify(value));
}

function propertyNames(value) {
  const names = new Set();
  let current = value;
  while (current && current !== Object.prototype) {
    for (const name of Object.getOwnPropertyNames(current)) names.add(name);
    current = Object.getPrototypeOf(current);
  }
  for (const name of manifest) {
    try {
      if (name in value) names.add(name);
    } catch (_) {}
  }
  return [...names].sort();
}

function serialize(value, seen = new WeakSet(), depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) return value;
  if (typeof value === "undefined") return { $undefined: true };
  if (typeof value === "bigint") return { $bigint: value.toString() };
  if (["function", "symbol"].includes(typeof value)) return { $unsupported: typeof value };
  if (depth > 12) return { $truncated: "max-depth" };
  if (
    typeof value === "object" &&
    "parent" in value &&
    typeof value.id === "string" &&
    typeof value.type === "string"
  ) {
    return { $nodeId: value.id, $nodeType: value.type };
  }
  if (Array.isArray(value)) return value.map((item) => serialize(item, seen, depth + 1));
  if (ArrayBuffer.isView(value)) {
    return { $binary: value.constructor.name, byteLength: value.byteLength };
  }
  if (value instanceof ArrayBuffer) return { $binary: "ArrayBuffer", byteLength: value.byteLength };
  if (seen.has(value)) return { $circular: true };
  seen.add(value);
  const result = {};
  for (const key of Object.keys(value).sort()) {
    try {
      const serialized = serialize(value[key], seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) result[key] = serialized;
    } catch (error) {
      result[key] = { $error: String(error && error.message ? error.message : error) };
    }
  }
  seen.delete(value);
  return result;
}

function truncatedValue(reason, byteLength) {
  return { $truncated: reason, byteLength };
}

function serializeField(nodeId, name, value, fieldErrors) {
  const serialized = serialize(value);
  const byteLength = jsonByteLength(serialized);
  if (byteLength <= maxFieldBytes) return serialized;
  const descriptor = devupLargeValueDescriptor(nodeId, name, serialized);
  if (descriptor.$truncated) {
    fieldErrors[name] = `DEVUP_FIELD_VALUE_UNSUPPORTED:${byteLength}>${DEVUP_MAX_LARGE_VALUE_BYTES}`;
  }
  return descriptor;
}

function isHardProtectedField(name) {
  return hardProtectedFields.has(name) || name.endsWith("StyleId");
}

function shrinkNodeToBudget(node, budget) {
  let currentBytes = jsonByteLength(node);
  if (currentBytes <= budget) return node;

  const candidates = [];
  for (const [sectionName, section] of [
    ["extra", node.extra],
    ["fields", node.fields],
  ]) {
    for (const [name, value] of Object.entries(section)) {
      if (value && typeof value === "object" && ("$truncated" in value || "$largeValue" in value)) {
        continue;
      }
      candidates.push({
        sectionName,
        name,
        byteLength: jsonByteLength(value),
        protected: sectionName === "fields" && isHardProtectedField(name),
      });
    }
  }
  candidates.sort(
    (left, right) =>
      Number(left.protected) - Number(right.protected) || right.byteLength - left.byteLength,
  );

  for (const candidate of candidates) {
    if (currentBytes <= budget) break;
    const descriptor = devupLargeValueDescriptor(
      node.id,
      candidate.name,
      node[candidate.sectionName][candidate.name],
    );
    node[candidate.sectionName][candidate.name] = descriptor;
    if (descriptor.$truncated) {
      node.fieldErrors[candidate.name] =
        `DEVUP_FIELD_VALUE_UNSUPPORTED:${candidate.byteLength}>${DEVUP_MAX_LARGE_VALUE_BYTES}`;
    }
    currentBytes = jsonByteLength(node);
  }
  return node;
}

// A variant's component properties are defined on its set, and Figma
// refuses to read them from the variant: "Can only get component property
// definitions of a component set or non-variant component".
function variantDefinitions(node, name) {
  return (
    name === "componentPropertyDefinitions" &&
    node.type === "COMPONENT" &&
    Boolean(node.parent) &&
    node.parent.type === "COMPONENT_SET"
  );
}

// Main-component identity is asynchronous under dynamic-page access. It is
// not a synchronous getter from the property manifest. Cache per collection
// so repacking the same page does not repeat its reads.
const devupComponentIdentity = new Map();
async function devupReadComponentIdentity(node, fields, fieldErrors) {
  if (node.type !== "INSTANCE" || typeof node.getMainComponentAsync !== "function") return;
  if (!devupComponentIdentity.has(node.id)) {
    devupComponentIdentity.set(node.id, (async () => {
      try {
        const main = await node.getMainComponentAsync();
        return { fields: {
          mainComponentId: main ? main.id : null,
          mainComponentName: main ? main.name : null,
          mainComponentSetId: main && main.parent && main.parent.type === "COMPONENT_SET" ? main.parent.id : null,
        } };
      } catch (error) {
        return { error: String(error && error.message ? error.message : error) };
      }
    })());
  }
  const identity = await devupComponentIdentity.get(node.id);
  if (identity.error) fieldErrors.mainComponentId = identity.error;
  else Object.assign(fields, identity.fields);
}
;
async function snapshotNode(node) {
  const fields = {};
  const extra = {};
  const fieldErrors = {};
  fields.parentId = node.parent ? node.parent.id : null;
  // Only the root needs this. Its parent lies outside the collected subtree,
  // so the id alone says nothing, and the parent's type is what decides
  // whether the root's width is a real constraint or merely the canvas the
  // design was drawn on. Every other node's parent is collected and can be
  // read directly.
  // Keyed on the parent's type rather than on being the requested root, so a
  // node carries the same fields however it is reached. See fast_snapshot.js.
  if (
    node.parent &&
    (node.parent.type === "PAGE" ||
      node.parent.type === "SECTION" ||
      node.parent.type === "COMPONENT_SET")
  ) {
    fields.parentType = node.parent.type;
    // A screen's Section names the page component the plugin writes for it,
    // `AboutPage` for a Section called `about`, and the Section itself is
    // outside the collected subtree.
    if (node.parent.type === "SECTION") fields.parentName = node.parent.name;
  }
  fields.childrenIds = "children" in node ? node.children.map((child) => child.id) : [];
  if (!("overflowDirection" in node) && ["FRAME", "COMPONENT", "INSTANCE", "COMPONENT_SET"].includes(node.type)) fields.overflowDirection = null;

  for (const name of propertyNames(node)) {
    if (skipped.has(name) || name.startsWith("_") || variantDefinitions(node, name)) continue;
    try {
      const value = node[name];
      if (typeof value === "function") continue;
      const serialized = serializeField(node.id, name, value, fieldErrors);
      (manifestSet.has(name) ? fields : extra)[name] = serialized;
    } catch (error) {
      fieldErrors[name] = String(error && error.message ? error.message : error);
    }
  }
  if (node.type === "TEXT" && typeof node.getStyledTextSegments === "function") {
    try {
      fields.styledTextSegments = serializeField(
        node.id,
        "styledTextSegments",
        node.getStyledTextSegments(textSegmentManifest),
        fieldErrors,
      );
    } catch (error) {
      fieldErrors.styledTextSegments = String(error && error.message ? error.message : error);
    }
  }
  await devupReadComponentIdentity(node, fields, fieldErrors);
  return { id: node.id, type: node.type, fields, extra, fieldErrors };
}

const allNodes = [];
const queue = [root];
while (queue.length) {
  const node = queue.shift();
  allNodes.push(node);
  if ("children" in node) queue.push(...node.children);
}

const nodes = [];
const nodeBudget = maxPayloadBytes - 1024;
let payloadBytes = 2;
for (let index = offset; index < allNodes.length; index += 1) {
  const node = shrinkNodeToBudget(await snapshotNode(allNodes[index]), nodeBudget);
  const nodeBytes = jsonByteLength(node) + (nodes.length ? 1 : 0);
  if (nodes.length && payloadBytes + nodeBytes > nodeBudget) break;
  nodes.push(node);
  payloadBytes += nodeBytes;
}

const nextOffset = Math.min(allNodes.length, offset + nodes.length);
nodes.push({
  id: "__DEVUP_SNAPSHOT_CURSOR__",
  type: "DEVUP_INTERNAL",
  // Same marker shape as the fast snapshot so both paths go through the one
  // `read_snapshot_cursor` reader in Rust.
  fields: {
    offset,
    nextOffset,
    complete: nextOffset >= allNodes.length,
    totalNodes: allNodes.length,
  },
  extra: {},
  fieldErrors: {},
});

return {
  fileKey: figma.fileKey || "",
  version: null,
  rootIds: [root.id],
  nodes,
  diagnostics: [],
};

}

/** source: used_resources.js */
async function usedResources(p) {
const resources = p.resources;

function serialize(value, seen = new WeakSet(), depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) return value;
  if (typeof value === "undefined") return { $undefined: true };
  if (typeof value === "bigint") return { $bigint: value.toString() };
  if (["function", "symbol"].includes(typeof value)) return { $unsupported: typeof value };
  if (depth > 12) return { $truncated: "max-depth" };
  if (typeof value === "object" && "parent" in value && typeof value.id === "string" && typeof value.type === "string") {
    return { $nodeId: value.id, $nodeType: value.type };
  }
  if (Array.isArray(value)) return value.map((item) => serialize(item, seen, depth + 1));
  if (ArrayBuffer.isView(value)) return { $binary: value.constructor.name, byteLength: value.byteLength };
  if (value instanceof ArrayBuffer) return { $binary: "ArrayBuffer", byteLength: value.byteLength };
  if (seen.has(value)) return { $circular: true };
  seen.add(value);
  const result = {};
  const names = new Set(Object.keys(value));
  let current = value;
  while (current && current !== Object.prototype) {
    for (const name of Object.getOwnPropertyNames(current)) names.add(name);
    current = Object.getPrototypeOf(current);
  }
  for (const name of [...names].sort()) {
    if (name.startsWith("_") || ["parent", "children", "consumers"].includes(name)) continue;
    try {
      const serialized = serialize(value[name], seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) result[name] = serialized;
    } catch (_error) {
      result[name] = { $error: "unavailable" };
    }
  }
  seen.delete(value);
  return result;
}

// 파일에 있는 변수는 한 번에 다 받아 두고 id 로 찾는다.
//
// 하나씩 `getVariableByIdAsync` 로 묻는 것이 이 파일에서는 건당 21.5초를 쓰고
// 끝내 null 을 냈다. 33개든 40개든 병렬이라 총 시간이 21.5초로 같았던 것이
// 단서였다 ? 작업이 아니라 건당 대기였다. 같은 순간 지역 목록을 받는 호출은
// 12ms 에 돌아온다. 그래서 그쪽을 먼저 쓴다.
const localVariableIndex = new Map();
let localVariableListing = "ok";
try {
  for (const variable of await figma.variables.getLocalVariablesAsync()) {
    localVariableIndex.set(variable.id, variable);
  }
} catch (_error) {
  localVariableListing = "unavailable";
}

function aliasIdsOf(variable) {
  return Object.values(variable.valuesByMode || {})
    .filter((value) => value && value.type === "VARIABLE_ALIAS" && typeof value.id === "string" && value.id)
    .map((value) => value.id);
}

async function resolveVariable(id) {
  const local = localVariableIndex.get(id);
  if (local) {
    return { value: serialize(local), collectionId: local.variableCollectionId, aliasIds: aliasIdsOf(local) };
  }
  // 목록에 없으면 이 파일 것이 아니다(라이브러리 변수). 그때만 개별로 묻는다.
  try {
    const value = await figma.variables.getVariableByIdAsync(id);
    return value
      ? { value: serialize(value), collectionId: value.variableCollectionId, aliasIds: aliasIdsOf(value) }
      : { unresolved: { id, kind: "variable", reason: "notInFileAndLookupEmpty" } };
  } catch (error) {
    return { unresolved: { id, kind: "variable", reason: "notInFileAndLookupThrew", error: String((error && error.message) || error).slice(0, 300) } };
  }
}

const variableResults = await Promise.all(resources.variableIds.map(resolveVariable));
// 변수는 어떤 노드도 묶지 않은 변수를 alias 로 가리킬 수 있다 — 라이브러리 primitive 를
// 가리키는 로컬 spacing 처럼. 테마가 그 alias 의 값을 내려면 그 변수가 있어야 하므로
// 그것도, 그것이 가리키는 것도 읽는다. 노드가 쓴 id 목록에는 넣지 않는다.
const requestedVariableIds = new Set(resources.variableIds);
let pendingAliasIds = variableResults.flatMap((result) => result.aliasIds || []);
while (pendingAliasIds.length > 0) {
  const batch = [...new Set(pendingAliasIds)]
    .filter((id) => !requestedVariableIds.has(id))
    .sort();
  for (const id of batch) requestedVariableIds.add(id);
  const fetched = await Promise.all(batch.map(resolveVariable));
  variableResults.push(...fetched);
  pendingAliasIds = fetched.flatMap((result) => result.aliasIds || []);
}

const collectionIds = [...new Set(variableResults
  .flatMap((result) => result.collectionId ? [result.collectionId] : []))].sort();
const collectionResults = await Promise.all(collectionIds.map(async (id) => {
  try {
    const collection = await figma.variables.getVariableCollectionByIdAsync(id);
    return collection ? [serialize(collection)] : [];
  } catch (_error) {
    return [];
  }
}));

const styleResults = await Promise.all(resources.styles.map(async (styleRef) => {
  try {
    const style = await figma.getStyleByIdAsync(styleRef.id);
    if (!style) {
      return { unresolved: { id: styleRef.id, kind: "style", reason: "notFoundOrUnavailable" } };
    }
    return {
      value: {
        ...serialize(style),
        styleType: styleRef.styleType,
        value: serialize(
          styleRef.styleType === "PAINT" ? style.paints
            : styleRef.styleType === "EFFECT" ? style.effects
              : styleRef.styleType === "GRID" ? style.layoutGrids
                : style
        )
      }
    };
  } catch (error) {
    return { unresolved: { id: styleRef.id, kind: "style", reason: "notFoundOrUnavailable", error: String((error && error.message) || error).slice(0, 300) } };
  }
}));

return {
  collections: collectionResults.flat(),
  variables: variableResults.flatMap((result) => result.value ? [result.value] : []),
  styles: styleResults.flatMap((result) => result.value ? [result.value] : []),
  usedVariableIds: resources.variableIds,
  usedStyleIds: resources.styles.map((style) => style.id),
  // 아무것도 해석되지 않았을 때 목록 자체를 못 받은 것인지 구분하기 위해 남긴다.
  localVariableListing,
  localVariableCount: localVariableIndex.size,
  unresolved: [...variableResults, ...styleResults]
    .flatMap((result) => result.unresolved ? [result.unresolved] : [])
};

}

/** source: variable_catalog.js */
async function variableCatalog(p) {
function serialize(value, seen = new WeakSet(), depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) return value;
  if (typeof value === "undefined") return { $undefined: true };
  if (typeof value === "bigint") return { $bigint: value.toString() };
  if (["function", "symbol"].includes(typeof value)) return { $unsupported: typeof value };
  if (depth > 12) return { $truncated: "max-depth" };
  if (Array.isArray(value)) return value.map((item) => serialize(item, seen, depth + 1));
  if (seen.has(value)) return { $circular: true };
  seen.add(value);
  const result = {};
  const names = new Set(Object.keys(value));
  let current = value;
  while (current && current !== Object.prototype) {
    for (const name of Object.getOwnPropertyNames(current)) names.add(name);
    current = Object.getPrototypeOf(current);
  }
  for (const name of [...names].sort()) {
    if (name.startsWith("_")) continue;
    try {
      const serialized = serialize(value[name], seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) result[name] = serialized;
    } catch (error) {
      result[name] = { $error: String(error && error.message ? error.message : error) };
    }
  }
  seen.delete(value);
  return result;
}

const [collections, paints, texts, effects, grids] = await Promise.all([
  figma.variables.getLocalVariableCollectionsAsync(),
  figma.getLocalPaintStylesAsync(),
  figma.getLocalTextStylesAsync(),
  figma.getLocalEffectStylesAsync(),
  figma.getLocalGridStylesAsync()
]);
const styleGroups = [paints, texts, effects, grids];
const styleTypes = ["PAINT", "TEXT", "EFFECT", "GRID"];

return {
  collections: collections.map((value) => serialize(value)),
  variableIds: [...new Set(collections.flatMap((collection) => collection.variableIds))].sort(),
  styles: styleGroups.flatMap((group, index) => group.map((style) => ({
    id: style.id,
    styleType: styleTypes[index]
  }))).sort((left, right) => left.id.localeCompare(right.id)),
  localComplete: true,
  usedRemoteComplete: false
};

}

/** source: variables.js */
async function variables(p) {
const resources = p.resources;

function serialize(value, seen = new WeakSet(), depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) return value;
  if (typeof value === "undefined") return { $undefined: true };
  if (typeof value === "bigint") return { $bigint: value.toString() };
  if (["function", "symbol"].includes(typeof value)) return { $unsupported: typeof value };
  if (depth > 12) return { $truncated: "max-depth" };
  if (typeof value === "object" && "parent" in value && typeof value.id === "string" && typeof value.type === "string") {
    return { $nodeId: value.id, $nodeType: value.type };
  }
  if (Array.isArray(value)) return value.map((item) => serialize(item, seen, depth + 1));
  if (ArrayBuffer.isView(value)) return { $binary: value.constructor.name, byteLength: value.byteLength };
  if (value instanceof ArrayBuffer) return { $binary: "ArrayBuffer", byteLength: value.byteLength };
  if (seen.has(value)) return { $circular: true };
  seen.add(value);
  const result = {};
  const names = new Set(Object.keys(value));
  let current = value;
  while (current && current !== Object.prototype) {
    for (const name of Object.getOwnPropertyNames(current)) names.add(name);
    current = Object.getPrototypeOf(current);
  }
  for (const name of [...names].sort()) {
    if (name.startsWith("_") || ["parent", "children", "consumers"].includes(name)) continue;
    try {
      const serialized = serialize(value[name], seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) result[name] = serialized;
    } catch (error) {
      result[name] = { $error: String(error && error.message ? error.message : error) };
    }
  }
  seen.delete(value);
  return result;
}

const [variableValues, styleValues] = await Promise.all([
  Promise.all(resources.variableIds.map((id) => figma.variables.getVariableByIdAsync(id))),
  Promise.all(resources.styles.map((style) => figma.getStyleByIdAsync(style.id)))
]);
const styleRefs = new Map(resources.styles.map((style) => [style.id, style]));
const styles = await Promise.all(styleValues.filter(Boolean).map(async (style) => {
  const styleRef = styleRefs.get(style.id);
  const styleType = styleRef.styleType;
  if (Number.isInteger(styleRef.consumerStart) && Number.isInteger(styleRef.consumerEnd)) {
    const consumers = await style.getStyleConsumersAsync();
    return {
      id: style.id,
      styleType,
      $consumerStart: styleRef.consumerStart,
      $consumerEntries: consumers.slice(styleRef.consumerStart, styleRef.consumerEnd).map((consumer) => [
        consumer.node.id,
        consumer.node.type,
        serialize(consumer.fields)
      ])
    };
  }
  const consumers = await style.getStyleConsumersAsync();
  return {
    ...serialize(style),
    styleType,
    $consumerCount: consumers.length,
    value: serialize(
      styleType === "PAINT" ? style.paints
        : styleType === "EFFECT" ? style.effects
          : styleType === "GRID" ? style.layoutGrids
            : style
    )
  };
}));

return {
  variables: variableValues.filter(Boolean).map((value) => serialize(value)),
  styles
};

}

return {
  assets,
  bridgeContext,
  explore,
  fastSnapshot,
  fastTheme,
  largeValue,
  measureNodes,
  metadata,
  pageCatalog,
  referencePng,
  referencePngChunk,
  search,
  sectionIndex,
  snapshot,
  usedResources,
  variableCatalog,
  variables,
};
})
