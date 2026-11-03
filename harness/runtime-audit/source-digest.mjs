import {createHash} from 'node:crypto';
import {readdir, lstat, readFile, realpath} from 'node:fs/promises';
import {resolve, relative, extname, join} from 'node:path';

export const SOURCE_SKIP = new Set(['.git','.devup','node_modules','target','dist','build','.next','df','out','.playwright','playwright-report','test-results','coverage','.cache','.agents','.codex','.claude','.opencode','.venv','__pycache__']);
export const SOURCE_EXTENSIONS = new Set(['ts','tsx','js','jsx','mjs','cjs','json','rs','toml','lock','html','svg','png','jpg','jpeg','webp','gif','pdf','woff','woff2','otf','ttf','sql','prisma','graphql','yaml','yml','md','mdx','css','scss','avif','ico','webm','mp4']);
export const digestBytes = bytes => createHash('sha256').update(bytes).digest('hex');

// Matches completion::source_digest. Dependencies, receipts and mutable DB
// contents are excluded; schema files and served media remain part of the input.
export async function sourceDigest(projectRoot) {
  const root=await realpath(resolve(projectRoot)), files={}, paths=[];
  let total=0;
  async function visit(dir) {
    for(const entry of await readdir(dir,{withFileTypes:true})) {
      if(SOURCE_SKIP.has(entry.name) || entry.name.startsWith('.env') || entry.name.endsWith('.log')) continue;
      const path=join(dir,entry.name), metadata=await lstat(path);
      if(metadata.isSymbolicLink()) throw new Error(`Source digest refuses symbolic links: ${relative(root,path)}`);
      if(metadata.isDirectory()) await visit(path);
      else if(metadata.isFile() && (SOURCE_EXTENSIONS.has(extname(path).slice(1).toLowerCase()) || relative(root,path).split(/[\\/]/).includes('public'))) {
        if(paths.length>=10000 || metadata.size>16*1024*1024 || total+metadata.size>128*1024*1024) throw new Error('Source digest bounds exceeded; no partial project digest was produced');
        total+=metadata.size;
        paths.push([relative(root,path).replaceAll('\\','/'),path]);
      }
    }
  }
  await visit(root);
  paths.sort((a,b)=>Buffer.compare(Buffer.from(a[0]),Buffer.from(b[0])));
  for(const [name,path] of paths) files[name]=digestBytes(await readFile(path));
  return {sourceDigest:digestBytes(JSON.stringify(files)),files};
}
