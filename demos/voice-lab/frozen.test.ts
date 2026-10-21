import {test,expect} from 'bun:test';
import {mkdtemp,readFile,writeFile,readdir} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {createApp,digest,validWave} from './core';
import {captureConditioning} from './frozen';
const recipe={base:'A',axes:[0.3,-0.2],space:'a'.repeat(64)};
function tone(){const n=48000,b=Buffer.alloc(44+n*4);b.write('RIFF');b.writeUInt32LE(b.length-8,4);b.write('WAVEfmt ',8);b.writeUInt32LE(16,16);b.writeUInt16LE(3,20);b.writeUInt16LE(1,22);b.writeUInt32LE(24000,24);b.writeUInt32LE(96000,28);b.writeUInt16LE(4,32);b.writeUInt16LE(32,34);b.write('data',36);b.writeUInt32LE(n*4,40);for(let i=0;i<n;i++)b.writeFloatLE(.01*Math.sin(i*.1),44+i*4);return b;}
const wave=tone();
const info={space:recipe.space,model:{revision:'b'.repeat(64)},dimension:3,text:'Synthetic test text.',source_vectors_sha256:'source-test',capabilities:{live_preview:true,frozen_conditioning:true,registry_promote:true}};
function nativeDocument(){return JSON.stringify({version:1,text:info.text,language:'german',reference_sha256:digest(wave),conditioning:{version:1,scope:{backend:'ggml',model_sha256:info.model.revision,dimension:3,representation:'raw-f32'},values:[2.3,-.2,7],recipe:{space:recipe.space,base:'A',offsets:recipe.axes}},pca:{components:[[1,0,0],[0,1,0]]}}).replace('[[1,0,0]','[[1,-0.0,0]');}
function request(path:string,body?:unknown,origin:string|null='http://127.0.0.1:8804'){return new Request('http://127.0.0.1:8804'+path,{method:body?'POST':'GET',headers:{Host:'127.0.0.1:8804',...(body?{'Content-Type':'application/json',...(origin?{Origin:origin}:{})}:{})},body:body?JSON.stringify(body):undefined});}
async function setup(promote?:any,missing=false){
 const data=await mkdtemp(join(tmpdir(),'spqx-frozen-'));let renders=0;
 const options={origin:'http://127.0.0.1:8804',data,info,publicDir:import.meta.dir+'/public',media:new Map<string,string>(),allowedRoots:[data],maxDaily:2,promote:promote||(async()=> 'vox-fixture'),render:async(_recipe:any,_id:any,admitted:any)=>{renders++;await admitted();return {wave,seconds:2,model:info.model.revision,basis:recipe.space,...(missing?{}:{native_voice_json:nativeDocument()})};}};
 return {data,app:createApp(options),reopen:()=>createApp(options),renders:()=>renders};
}
async function preview(app:any){const response=await app(request('/api/preview',recipe));expect(response.status).toBe(202);let job=await response.json();for(let i=0;i<80&&['running','queued'].includes(job.status);i++){await Bun.sleep(5);job=await(await app(request('/api/jobs/'+job.id))).json();}return job;}

test('captures exact numeric bytes including minus-zero; no browser re-serialization',()=>{
 const raw=nativeDocument(),captured=captureConditioning({native_voice_json:raw},recipe,info,digest(wave));expect(captured?.raw).toBe(raw);expect(captured?.raw).toContain('-0.0');
 expect(()=>captureConditioning({native_voice_json:raw}, {...recipe,axes:[0.35,-.2]},info,digest(wave))).toThrow();
 expect(()=>captureConditioning({native_voice_json:raw},recipe,{...info,model:{revision:'other'}},digest(wave))).toThrow();
 expect(()=>captureConditioning({native_voice_json:raw},recipe,info,'different-wave')).toThrow();
});
test('SDK draft records effective bf16 values and cannot masquerade as native',()=>{
 const raw=JSON.stringify({version:1,provider:'qwen-sdk',model:info.model,basis_sha256:recipe.space,source_vectors_sha256:info.source_vectors_sha256,representation:'effective-bfloat16-as-f32',recipe,values:[.5,-.25,1]});
 expect(captureConditioning({sdk_conditioning_json:raw},recipe,info,digest(wave))?.kind).toBe('sdk');
 expect(()=>captureConditioning({sdk_conditioning_json:raw,native_voice_json:nativeDocument()},recipe,info,digest(wave))).toThrow();
});
test('native WAV extensible float is accepted without changing the audition bytes',()=>{
 const extended=Buffer.alloc(wave.length+24);wave.copy(extended,0,0,12);extended.writeUInt32LE(extended.length-8,4);extended.write('fmt ',12);extended.writeUInt32LE(40,16);wave.copy(extended,20,20,36);extended.writeUInt16LE(65534,20);extended.writeUInt16LE(22,36);extended.writeUInt16LE(32,38);extended.writeUInt32LE(1,40);Buffer.from('0300000000001000800000aa00389b71','hex').copy(extended,44);extended.write('data',60);extended.writeUInt32LE(48000*4,64);wave.copy(extended,68,44);
 expect(validWave(extended,2)).toBe(true);extended[44]=1;expect(validWave(extended,2)).toBe(false);
});
test('completed preview stores exact WAV/vector before explicit canonical saving; retries do not render or promote twice',async()=>{
 let promotions=0,seenRaw='';const env=await setup(async(directory:string)=>{promotions++;seenRaw=await readFile(join(directory,'voice.json'),'utf8');return 'vox-fixture';});
 const job=await preview(env.app);expect(job.status).toBe('ready');expect(promotions).toBe(0);
 const body={job:job.id,name:'Synthetic canonical fixture'};
 const response=await env.app(request('/api/voices',body));expect(response.status).toBe(201);const saved=await response.json();
 expect(saved.canonical_voice_id).toBe('vox-fixture');expect(saved.registry_promoted).toBe(true);expect(seenRaw).toBe(nativeDocument());expect(await readFile(join(env.data,'drafts',saved.id,'reference.wav'))).toEqual(wave);expect(await readFile(join(env.data,'drafts',saved.id,'reference.txt'),'utf8')).toBe(info.text);
 expect((await env.app(request('/api/voices',body))).status).toBe(201);expect(promotions).toBe(1);expect(env.renders()).toBe(1);
 const state=await(await env.reopen()(request('/api/state'))).json();expect(state.drafts[0].voice_id).toBe('vox-fixture');
 expect((await env.app(request('/api/voices',body,null))).status).toBe(403);expect((await env.app(request('/api/voices',{...body,registry:'/arbitrary'}))).status).toBe(400);
});
test('promotion failure retains exact draft and retry operation id, not another generated take',async()=>{
 const ids:string[]=[];const env=await setup(async(_directory:string,_name:string,id:string)=>{ids.push(id);if(ids.length===1)throw new Error('isolated registry temporarily unavailable');return 'vox-retry';});
 const job=await preview(env.app),body={job:job.id,name:'Retry fixture'};
 expect((await env.app(request('/api/voices',body))).status).toBe(503);
 expect((await env.app(request('/api/voices',body))).status).toBe(201);expect(ids[0]).toBe(ids[1]);expect(env.renders()).toBe(1);expect((await readdir(join(env.data,'drafts'))).length).toBe(1);
});
test('changed frozen record is rejected rather than saved with stale audio or regenerated',async()=>{
 const env=await setup(),job=await preview(env.app);await writeFile(join(env.data,'jobs',job.id+'.native.json'),nativeDocument()+' ');
 expect((await env.app(request('/api/voices',{job:job.id,name:'Corrupt fixture'}))).status).toBe(409);expect(env.renders()).toBe(1);
});
test('missing mandatory vector fails the preview instead of claiming a complete reusable voice',async()=>{
 const env=await setup(undefined,true),job=await preview(env.app);expect(job.status).toBe('failed');expect(job.error).toBe('missing_conditioning');expect((await readdir(join(env.data,'jobs'))).some(name=>name.endsWith('.wav'))).toBe(false);
});
