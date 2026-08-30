function serialize(value, seen = new WeakSet(), depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) return value;
  if (typeof value === "undefined") return null;
  if (["function", "symbol"].includes(typeof value)) return undefined;
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
    if (name.startsWith("_") || ["parent", "children"].includes(name)) continue;
    try {
      const serialized = serialize(value[name], seen, depth + 1);
      if (typeof serialized !== "undefined") result[name] = serialized;
    } catch (error) {
      result[name] = { $error: String(error && error.message ? error.message : error) };
    }
  }
  seen.delete(value);
  return result;
}

const collections = (await figma.variables.getLocalVariableCollectionsAsync()).map(serialize);
const variables = (await figma.variables.getLocalVariablesAsync()).map(serialize);
const styleGroups = await Promise.all([
  figma.getLocalPaintStylesAsync(),
  figma.getLocalTextStylesAsync(),
  figma.getLocalEffectStylesAsync(),
  figma.getLocalGridStylesAsync()
]);
const styleTypes = ["PAINT", "TEXT", "EFFECT", "GRID"];
const styles = styleGroups.flatMap((group, index) => group.map((style) => ({
  ...serialize(style),
  styleType: styleTypes[index],
  value: serialize(
    styleTypes[index] === "PAINT" ? style.paints
      : styleTypes[index] === "EFFECT" ? style.effects
        : styleTypes[index] === "GRID" ? style.layoutGrids
          : style
  )
})));

return {
  collections,
  variables,
  styles,
  usedRemoteVariables: [],
  localComplete: true
};
