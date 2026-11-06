import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdtemp,writeFile,mkdir,rm} from 'node:fs/promises';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {recordProjectChecks} from '../record-project-checks.mjs';

test('project command evidence records real failure and rejects stale source',async()=>{
  const dir=await mkdtemp(join(tmpdir(),'devup-command-receipt-'));
  try{
    await mkdir(join(dir,'src'));await writeFile(join(dir,'src/page.tsx'),'export const page=1;');
    const receipt=await recordProjectChecks({projectRoot:dir,checks:[
      {name:'build',command:[process.execPath,'-e','process.exit(7)']},
      {name:'test',command:[process.execPath,'-e',"require('node:fs').writeFileSync('src/page.tsx','export const changed=2;')"]},
    ]});
    assert.equal(receipt.checks[0].exitCode,7);assert.equal(receipt.sourceChanged,true);
    await assert.rejects(recordProjectChecks({projectRoot:dir,checks:[{name:'build',command:[]}]}));
  }finally{await rm(dir,{recursive:true,force:true});}
});

test('functional e2e command has separate observed evidence from unit tests',async()=>{
  const dir=await mkdtemp(join(tmpdir(),'devup-e2e-receipt-'));
  try{
    await writeFile(join(dir,'package.json'),'{}');
    const receipt=await recordProjectChecks({projectRoot:dir,checks:[
      {name:'test',command:[process.execPath,'-e','process.exit(0)']},
      {name:'e2e',command:[process.execPath,'-e','process.exit(9)']},
    ]});
    assert.deepEqual(receipt.checks.map(({name,exitCode})=>({name,exitCode})),[{name:'test',exitCode:0},{name:'e2e',exitCode:9}]);
    assert.equal(receipt.sourceChanged,false);
  }finally{await rm(dir,{recursive:true,force:true});}
});
