import { mkdtemp, mkdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
const hash=(v:Uint8Array|string)=>createHash('sha256').update(v).digest('hex');
/** Generated tones only: this fixture exercises the real file/API paths, not voice quality. */
export async function curationFixture(){
 const root=await mkdtemp(join(tmpdir(),'spqx-curation-')),rows=[];
 const wave=Buffer.alloc(44+24000*4*3);wave.write('RIFF');wave.writeUInt32LE(wave.length-8,4);wave.write('WAVEfmt ',8);wave.writeUInt32LE(16,16);wave.writeUInt16LE(3,20);wave.writeUInt16LE(1,22);wave.writeUInt32LE(24000,24);wave.writeUInt32LE(96000,28);wave.writeUInt16LE(4,32);wave.writeUInt16LE(32,34);wave.write('data',36);wave.writeUInt32LE(wave.length-44,40);
 for(let i=0;i<72000;i++)wave.writeFloatLE(.1*Math.sin(i*2*Math.PI*220/24000),44+i*4);
 for(const [i,id] of ['c001','c002'].entries()){
  const dir=join(root,'candidates',id);await mkdir(join(dir,'audition'),{recursive:true});
  await writeFile(join(dir,'audition/reference.wav'),wave);
  const encoder=Bun.spawn(['ffmpeg','-v','error','-n','-i',join(dir,'audition/reference.wav'),'-c:a','aac','-b:a','64k',join(dir,'listening.m4a')],{stdout:'ignore',stderr:'pipe'});
  if(await encoder.exited!==0)throw new Error('Fixture AAC encoder failed: '+await new Response(encoder.stderr).text());
  const vector=Buffer.alloc(4096),values=Array.from({length:1024},(_,j)=>(j%7+i)/16);values.forEach((v,j)=>vector.writeFloatLE(v,j*4));
  const voice=JSON.stringify({version:1,text:'Synthetische Testaufnahme.',reference_sha256:hash(wave),conditioning:{values}})+'\n';await writeFile(join(dir,'audition/voice.json'),voice);
  rows.push({id,seconds:3,audio_sha256:hash(wave),listening_sha256:hash(await Bun.file(join(dir,'listening.m4a')).bytes()),vector_f32_sha256:hash(vector),conditioning_sha256:hash(voice)});
 }
 await writeFile(join(root,'index.json'),JSON.stringify({version:1,id:'synthetic-curation-fixture',text:'Synthetische Testaufnahme.',ready:rows}));
 return {root,rows};
}
