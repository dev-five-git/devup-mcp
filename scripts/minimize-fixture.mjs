// Retain a reduction only when complete production codegen evidence is equal:
// TSX, source maps, diagnostics, traces, fidelity and responsive merge contracts.
import {readFile,writeFile,mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
import {pathToFileURL} from 'node:url';

export async function minimizeFixture({binary,input,output,maxAttempts=300}){
  const original=await readFile(input),fixture=JSON.parse(original),dir=await mkdtemp(join(tmpdir(),'devup-minimize-'));
  const before={bytes:original.length,nodes:Object.keys(fixture.nodes).length};let attempts=0,removed=0;
  try{
    const capture=join(dir,'capture.json'),tsx=join(dir,'Generated.tsx'),proof=join(dir,'proof.txt');
    async function project(){
      await writeFile(capture,JSON.stringify(fixture));
      const result=spawnSync(binary,[capture,tsx,'--clean','--proof',proof],{windowsHide:true,encoding:'utf8',timeout:60_000});
      if(result.status!==0)return null;
      return readFile(proof,'utf8');
    }
    const baseline=await project();if(!baseline)throw new Error('Original fixture cannot be projected');
    async function remove(object,key){
      if(attempts++>=maxAttempts)return;const value=object[key];delete object[key];
      if(await project()===baseline)removed++;else object[key]=value;
    }
    const roots=new Set(fixture.roots);
    for(const id of Object.keys(fixture.nodes))if(!roots.has(id)&&attempts<maxAttempts)await remove(fixture.nodes,id);
    for(const node of Object.values(fixture.nodes)){
      for(const container of [node.extra,node.fieldErrors,node.fields]){
        if(!container)continue;
        for(const key of Object.keys(container)){
          if(['id','parentId','childrenIds','name'].includes(key))continue;
          if(attempts>=maxAttempts)break;
          await remove(container,key);
        }
      }
      if(attempts>=maxAttempts)break;
    }
    if(await project()!==baseline)throw new Error('Reduced evidence differs; no output written');
    const bytes=Buffer.from(`${JSON.stringify(fixture,null,2)}\n`);await writeFile(output,bytes);
    return {before,after:{bytes:bytes.length,nodes:Object.keys(fixture.nodes).length},attempts,removed,evidence:'complete-production-codegen-debug-structure-identical'};
  }finally{await rm(dir,{recursive:true,force:true});}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
  const [binary,input,output]=process.argv.slice(2);console.log(JSON.stringify(await minimizeFixture({binary,input,output,maxAttempts:Number(process.argv[5]??300)})));
}
