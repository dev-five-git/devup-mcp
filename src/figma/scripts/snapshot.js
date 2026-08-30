const node = await figma.getNodeByIdAsync("__DEVUP_NODE_ID__");
if (!node) throw new Error("DEVUP_NODE_NOT_FOUND");
return { id: node.id, type: node.type, name: node.name };

