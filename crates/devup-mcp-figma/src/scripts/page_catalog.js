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
