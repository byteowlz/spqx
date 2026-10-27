import { readFile,writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
const hash=(v:Uint8Array)=>createHash('sha256').update(v).digest('hex');
/** A distinct synthetic tone source, not evidence of a model-designed voice. */
export async function addDesignFixture(root:string){
 const file=join(root,'index.json'),index=JSON.parse(await readFile(file,'utf8'));
 index.id='synthetic-character-fixture';index.title='Fantasy fixture';
 for(const c of index.ready){
  const dir=join(root,'candidates',c.id),wave=await readFile(join(dir,'audition/reference.wav'));
  for(let i=44;i<wave.length;i+=4)wave.writeFloatLE(wave.readFloatLE(i)*.5,i);
  await writeFile(join(dir,'reference.wav'),wave);
  const p=Bun.spawn(['ffmpeg','-v','error','-n','-i',join(dir,'reference.wav'),'-c:a','aac','-b:a','64k',join(dir,'design-listening.m4a')],{stdout:'ignore',stderr:'pipe'});
  if(await p.exited!==0)throw new Error('Synthetic source AAC failed');
  c.design_seconds=c.seconds;c.design_audio_sha256=hash(wave);c.design_listening_sha256=hash(await Bun.file(join(dir,'design-listening.m4a')).bytes());
  const receipt=Buffer.from(JSON.stringify({output_sha256:c.design_audio_sha256,result:{operation:'voice_design',eos_reached:true,text:index.text,description:'Original adult clockwork '+c.id+'. <em>Literal prompt characters, not markup.</em>'}}));
  await writeFile(join(dir,'design.json'),receipt);c.design_receipt_sha256=hash(receipt);
 }
 await writeFile(file,JSON.stringify(index));return index;
}
