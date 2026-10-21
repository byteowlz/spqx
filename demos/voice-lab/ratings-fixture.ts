import { createHash } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
export async function ratingFixture(root:string,listening=Buffer.from('synthetic listening bytes')){
  const digest=(v:Uint8Array)=>createHash('sha256').update(v).digest('hex');
  const raw=Buffer.from('synthetic raw audition; not speech');
  const ready=[];
  for(const id of ['c001','c002']){
    const directory=join(root,'candidates',id);await mkdir(join(directory,'audition'),{recursive:true});
    await writeFile(join(directory,'listening.m4a'),listening);await writeFile(join(directory,'audition/reference.wav'),raw);
    ready.push({id,seconds:2,audio_sha256:digest(raw),listening_sha256:digest(listening),description:'HIDDEN REQUESTED PROMPT'});
  }
  await writeFile(join(root,'index.json'),JSON.stringify({version:1,id:'synthetic-test-bank',mode:'vector_only',text:'Synthetic review fixture. Not a real speaker.',target_count:2,status:'complete',ready,description:'HIDDEN REQUESTED PROMPT'}));
  return {ready,raw,listening};
}
