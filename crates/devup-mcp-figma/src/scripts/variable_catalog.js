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
