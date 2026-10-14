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
