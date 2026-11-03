import {spawnSync} from 'node:child_process';
import {dirname,resolve,join} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {readdirSync} from 'node:fs';

const harness=dirname(fileURLToPath(import.meta.url));
function command(file,args,cwd=harness){
  const result=spawnSync(file,args,{cwd,stdio:'inherit',windowsHide:true,env:{...process.env,DEVUP_BROWSER_CI_CHILD:'1'}});
  if(result.error||result.status!==0) throw new Error(`CI browser stage failed: ${file} ${args.join(' ')} (${result.error?.message??result.status})`);
}
// The existing CI verification entry calls this hook on Linux. Keep the full
// browser gate here so a static checks-only pass can never substitute for it.
export function runBrowserCI(){
  command('npm',['ci']);
  command(process.execPath,[join(harness,'node_modules/playwright/cli.js'),'install','--with-deps','chromium']);
  command(process.execPath,[join(harness,'prepared-binaries.mjs')]);
  const files=readdirSync(join(harness,'test')).filter(name=>name.endsWith('.test.mjs')).sort().map(name=>join(harness,'test',name));
  command(process.execPath,['--test','--test-concurrency=1',...files]);
}
export const shouldRunBrowserCI=()=>process.env.GITHUB_ACTIONS==='true'&&process.platform==='linux'&&process.env.DEVUP_BROWSER_CI_CHILD!=='1';
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href) runBrowserCI();
