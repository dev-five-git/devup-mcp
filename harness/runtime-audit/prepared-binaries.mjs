import {execFile,spawnSync} from 'node:child_process';
import {promisify} from 'node:util';
import {readFile,writeFile,mkdir,readdir,stat} from 'node:fs/promises';
import {resolve,join,dirname,relative} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';

const execute=promisify(execFile), harness=dirname(fileURLToPath(import.meta.url)), repo=resolve(harness,'../..');
let preparing;
async function fingerprint(){
  const names=['Cargo.toml','Cargo.lock','rust-toolchain.toml'];
  async function visit(dir){
    for(const item of await readdir(dir,{withFileTypes:true})){
      const path=join(dir,item.name), name=relative(repo,path).replaceAll('\\','/');
      if(item.isDirectory()) {if(!['target','node_modules'].includes(item.name)) await visit(path);}
      else if(name.includes('/src/')||name.includes('/examples/')||item.name==='Cargo.toml'||item.name==='build.rs'||item.name==='build_identity.rs') names.push(name);
    }
  }
  await visit(join(repo,'crates'));
  for(const name of await readdir(harness)) if(name.endsWith('.mjs')||['package.json','package-lock.json'].includes(name)) names.push(`harness/runtime-audit/${name}`);
  const hash=createHash('sha256');
  for(const name of names.sort()) {hash.update(name);hash.update(await readFile(join(repo,name)));}
  return hash.digest('hex');
}

// Build once before any browser test starts. Per-check timeouts measure the
// application, not a Cargo build lock held by another Rust test command.
export function preparedBinaries(){
  return preparing ??= (async()=>{
    const out=join(harness,'out');await mkdir(out,{recursive:true});
    const receiptPath=join(out,'prepared-binaries.json'), sourceHash=await fingerprint();
    let receipt;try{receipt=JSON.parse(await readFile(receiptPath,'utf8'));}catch{}
    if(receipt?.sourceHash===sourceHash&&receipt.platform===process.platform&&await Promise.all(Object.values(receipt.paths).map(path=>stat(path).then(s=>s.isFile(),()=>false))).then(values=>values.every(Boolean))) return receipt.paths;
    const options={cwd:repo,windowsHide:true,timeout:20*60_000,maxBuffer:32*1024*1024,encoding:'utf8'};
    try{
      const metadata=JSON.parse((await execute('cargo',['metadata','--locked','--format-version','1','--no-deps'],options)).stdout);
      const build=await execute('cargo',['build','--locked','-j','2','-p','devup-mcp','--bin','devup-mcp','-p','devup-mcp-visual','--bin','devup-mcp-visual','-p','devup-mcp-devup-ui','--example','render_fixture'],options);
      await writeFile(join(out,'binary-preparation.log'),`${build.stdout}\n${build.stderr}`);
      const ext=process.platform==='win32'?'.exe':'';
      const paths={mcp:join(metadata.target_directory,'debug',`devup-mcp${ext}`),visual:join(metadata.target_directory,'debug',`devup-mcp-visual${ext}`),fixture:join(metadata.target_directory,'debug/examples',`render_fixture${ext}`)};
      const after=await fingerprint();
      if(sourceHash!==after) throw new Error('Rust/template source changed during preparation; finish edits before browser verification');
      await writeFile(receiptPath,JSON.stringify({sourceHash,platform:process.platform,paths},null,2));return paths;
    }catch(error){throw new Error(`Tooling preparation failed before browser assertions: ${error.message}\n${error.stderr??''}`,{cause:error});}
  })();
}

export async function renderFixture(snapshot,output,{responsive=false,clean=false,theme}={}){
  const paths=await preparedBinaries();
  const args=[snapshot,output,...(responsive?['--responsive']:[]),...(clean?['--clean']:[]),...(theme?['--theme',theme]:[])];
  const result=spawnSync(paths.fixture,args,{cwd:repo,encoding:'utf8',windowsHide:true,timeout:60_000});
  if(result.error) throw new Error(`Fixture execution failed after successful preparation: ${result.error.message}`);
  return result;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
  console.log(JSON.stringify(await preparedBinaries()));
}
