import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
// Reproduces the pre-category save exactly, so compatibility is tested against bytes
// the shipped rater really wrote rather than an invented shape.
export async function writeLegacyRating(root:string,id:string,audio_sha256:string,revision=1){
  const digest=(v:Uint8Array|string)=>createHash('sha256').update(v).digest('hex');
  const value={bank:'synthetic-test-bank',audio_sha256,operation_id:randomUUID(),expected_revision:revision-1,scores:Object.fromEntries(['pitch','brightness','breathiness','roughness','weight','nasality'].map(t=>[t,50])),naturalness:70,excluded:false,pronunciation_issue:false,notes:'legacy synthetic rating'};
  const rating={...value,version:1,candidate:id,revision,updated:new Date().toISOString(),source:'explicit-rating' as const,fingerprint:digest(JSON.stringify(value))};
  await mkdir(join(root,'ratings'),{recursive:true});await writeFile(join(root,'ratings',id+'.json'),JSON.stringify(rating,null,2)+'\n',{mode:0o600});
  return {rating,bytes:await readFile(join(root,'ratings',id+'.json'))};
}
export async function ratingFixture(root:string,listening=Buffer.from('synthetic listening bytes'),extra:Record<string,unknown>={}){
  const digest=(v:Uint8Array)=>createHash('sha256').update(v).digest('hex');
  const raw=Buffer.from('synthetic raw audition; not speech');
  const ready=[];
  for(const id of ['c001','c002']){
    const directory=join(root,'candidates',id);await mkdir(join(directory,'audition'),{recursive:true});
    await writeFile(join(directory,'listening.m4a'),listening);await writeFile(join(directory,'audition/reference.wav'),raw);
    ready.push({id,seconds:2,audio_sha256:digest(raw),listening_sha256:digest(listening),description:'HIDDEN REQUESTED PROMPT',requested_presentation:'HIDDEN REQUESTED MASCULINE',requested_age:'HIDDEN REQUESTED OLDER'});
  }
  await writeFile(join(root,'index.json'),JSON.stringify({version:1,id:'synthetic-test-bank',mode:'vector_only',text:'Synthetic review fixture. Not a real speaker.',target_count:2,status:'complete',ready,description:'HIDDEN REQUESTED PROMPT',...extra}));
  return {ready,raw,listening};
}
