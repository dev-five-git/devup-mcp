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
