import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {readFile,writeFile,mkdir,realpath} from 'node:fs/promises';
import {resolve,dirname,join} from 'node:path';
import {pathToFileURL} from 'node:url';
import {sourceDigest} from './source-digest.mjs';
const execute=promisify(execFile);

export async function recordProjectChecks({projectRoot,checks,out}){
  const root=await realpath(resolve(projectRoot));
  const names=new Set();
  if(!Array.isArray(checks)||checks.length>12||checks.some(check=>{
    if(!['build','lint','typecheck','test','e2e'].includes(check.name)||names.has(check.name)) return true;
    names.add(check.name);
    return !Array.isArray(check.command)||!check.command.length||check.command.length>64||check.command.some(arg=>typeof arg!=='string'||arg.includes('\0'));
  })) throw new Error('Checks require unique build/lint/typecheck/test/e2e names and explicit command argv arrays');
  const before=await sourceDigest(root),results=[];
  for(const check of checks){
    const startedAt=new Date().toISOString();let exitCode=0,stderr='';
    try{await execute(check.command[0],check.command.slice(1),{cwd:root,windowsHide:true,timeout:10*60_000,maxBuffer:8*1024*1024});}
    catch(error){exitCode=Number.isInteger(error.code)?error.code:-1;stderr=String(error.stderr??error.message).slice(-4000);}
    results.push({name:check.name,command:check.command,exitCode,startedAt,finishedAt:new Date().toISOString(),stderr});
  }
  const after=await sourceDigest(root);
  const receipt={kind:'devup-project-check-receipt',schemaVersion:1,projectRoot:root,sourceDigest:before.sourceDigest,sourceChanged:before.sourceDigest!==after.sourceDigest,sourceFiles:before.files,checks:results,executor:{node:process.version,trust:'locally-executed-command-exits'}};
  const target=out?resolve(out):join(root,'.devup/checks.json');await mkdir(dirname(target),{recursive:true});await writeFile(target,`${JSON.stringify(receipt,null,2)}\n`);
  return receipt;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
  const config=JSON.parse(await readFile(process.argv[2],'utf8'));
  const receipt=await recordProjectChecks(config);
  console.log(JSON.stringify({sourceDigest:receipt.sourceDigest,sourceChanged:receipt.sourceChanged,checks:receipt.checks.map(({name,exitCode})=>({name,exitCode}))}));
  if(receipt.sourceChanged||receipt.checks.some(check=>check.exitCode!==0)) process.exitCode=1;
}
