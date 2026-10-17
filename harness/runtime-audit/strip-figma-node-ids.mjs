// A Vite plugin that removes data-figma-node-id and data-figma-node-ids from
// JSX in `vite build` (vinext build), so production HTML carries no audit
// attributes. They stay in `vite dev` and in an audit build:
// `DEVUP_AUDIT=1 bun run build`.
//
// It runs as a `pre` transform on .tsx/.jsx sources: Rolldown transforms
// TypeScript and JSX only after the transform hooks run, so the attributes
// are still plain JSX text here. devup-mcp writes them as double-quoted
// string literals, which is all this matches.
//
//   import stripFigmaNodeIds from './vite-plugin-strip-figma-node-ids.mjs'
//   export default { plugins: [stripFigmaNodeIds(), vinext()] }

const ATTRIBUTE = /\s+data-figma-node-ids?="[^"]*"/g;

/** The source with every data-figma-node-id(s) attribute removed. */
export function stripFigmaNodeIds(code) {
  return code.replace(ATTRIBUTE, '');
}

/** Whether a build keeps the attributes: an audit build, or a dev server. */
export function keepsAttributes(command, env = process.env) {
  return command !== 'build' || env.DEVUP_AUDIT === '1';
}

export default function stripFigmaNodeIdsPlugin(env = process.env) {
  return {
    name: 'devup-strip-figma-node-ids',
    enforce: 'pre',
    apply: (_config, { command }) => !keepsAttributes(command, env),
    transform(code, id) {
      if (!/\.[jt]sx$/.test(id.split('?')[0]) || !code.includes('data-figma-node-id')) return null;
      return { code: stripFigmaNodeIds(code), map: null };
    },
  };
}
