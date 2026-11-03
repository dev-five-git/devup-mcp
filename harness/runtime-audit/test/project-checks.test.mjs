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
