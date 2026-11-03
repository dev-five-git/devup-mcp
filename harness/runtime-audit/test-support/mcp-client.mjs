import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';

export async function mcpClient(binary,projectRoot){
  const child=spawn(binary,['--allow-write-root',projectRoot],{cwd:projectRoot,windowsHide:true,env:{...process.env,DEVUP_FIGMA_BRIDGE_PORT:'off',DEVUP_MCP_NO_AUTO_UPDATE:'1'},stdio:['pipe','pipe','pipe']});
  const pending=new Map();let next=0,stderr='';
  child.stderr.on('data',data=>{stderr=(stderr+data).slice(-4000);});
  createInterface({input:child.stdout}).on('line',line=>{
    let message;try{message=JSON.parse(line);}catch{return;}
    const entry=pending.get(message.id);if(!entry)return;pending.delete(message.id);clearTimeout(entry.timer);
    if(message.error)entry.reject(new Error(JSON.stringify(message.error)));else entry.resolve(message.result);
  });
  const request=(method,params)=>new Promise((resolve,reject)=>{
    const id=++next,timer=setTimeout(()=>{pending.delete(id);reject(new Error(`MCP request ${method} timed out: ${stderr}`));},30_000);
    pending.set(id,{resolve,reject,timer});child.stdin.write(`${JSON.stringify({jsonrpc:'2.0',id,method,params})}\n`);
  });
  child.on('exit',code=>{for(const entry of pending.values()){clearTimeout(entry.timer);entry.reject(new Error(`MCP exited ${code}: ${stderr}`));}pending.clear();});
  const initialized=await request('initialize',{protocolVersion:'2025-06-18',capabilities:{},clientInfo:{name:'codex-consumer-fixture',version:'1'}});
  child.stdin.write(`${JSON.stringify({jsonrpc:'2.0',method:'notifications/initialized'})}\n`);
  return {initialized,request,async call(name,args){
    const value=await request('tools/call',{name,arguments:args});
    if(value.isError)throw new Error(JSON.stringify(value));
    return value.structuredContent??JSON.parse(value.content.find(item=>item.type==='text').text);
  },async close(){
    if(child.exitCode!==null)return;
    await new Promise(done=>{
      const timer=setTimeout(()=>child.kill(),2000);
      child.once('exit',()=>{clearTimeout(timer);done();});
      child.stdin.end();
    });
  }};
}
