import { test,expect,afterEach } from 'bun:test';
import { readFile,writeFile,readdir,rm,symlink,cp } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { join } from 'node:path';
import { CurationBank } from './curation';
import { curationFixture } from './curation-fixture';
import { createApp } from './core';
const roots:string[]=[];afterEach(async()=>{await Promise.all(roots.splice(0).map(r=>rm(r,{recursive:true,force:true})));});
async function fixture(){const f=await curationFixture();roots.push(f.root);return {...f,bank:new CurationBank(f.root)};}
const input=(f:any,decision:any='keep',revision=0)=>({bank:'synthetic-curation-fixture',audio_sha256:f.rows[0].audio_sha256,expected_revision:revision,operation_id:randomUUID(),decision,notes:''});
test('read-only state leaves no review directory or fabricated decisions',async()=>{
 const f=await fixture();const s=await f.bank.state();expect(s.reviewed).toBe(0);expect(s.candidates.map(c=>c.review)).toEqual([null,null]);expect((await readdir(f.root)).sort()).toEqual(['candidates','index.json']);
});
test('explicit unsure, null notes-only, exact retry and previous revision are distinct',async()=>{
 const f=await fixture(),first=input(f,'unsure');const one=await f.bank.save('c001',first);expect(one.decision).toBe('unsure');expect(await f.bank.save('c001',first)).toEqual(one);
 await expect(f.bank.save('c001',{...first,notes:'different'})).rejects.toMatchObject({code:'operation_conflict'});
 const two=await f.bank.save('c001',{...input(f,null,1),notes:'Still considering'});expect(two.revision).toBe(2);expect((await f.bank.state()).reviewed).toBe(0);
 expect(JSON.parse(await readFile(join(f.root,'curation-reviews/c001.previous.json'),'utf8')).decision).toBe('unsure');
});
test('concurrent/stale intents cannot overwrite a saved review',async()=>{
 const f=await fixture(),a=input(f),b=input(f,'reject');const r=await Promise.allSettled([f.bank.save('c001',a),f.bank.save('c001',b)]);
 expect(r.filter(x=>x.status==='fulfilled')).toHaveLength(1);expect((await f.bank.state()).reviewed).toBe(1);
 await expect(f.bank.save('c001',input(f,'reject'))).rejects.toMatchObject({code:'revision_conflict'});
});
test('invalid values, unknown paths, changed raw audio and conditioning fail closed',async()=>{
 const f=await fixture();await expect(f.bank.save('c001',{...input(f),score:50})).rejects.toMatchObject({code:'invalid_review'});
 await expect(f.bank.save('../c001',input(f))).rejects.toMatchObject({code:'not_found'});
 await expect(f.bank.save('c001',{...input(f),bank:'other'})).rejects.toMatchObject({code:'audio_changed'});
 await writeFile(join(f.root,'candidates/c001/audition/reference.wav'),'changed');
 await expect(f.bank.save('c001',input(f))).rejects.toMatchObject({code:'audio_changed'});
 await expect(f.bank.state()).resolves.toMatchObject({reviewed:0});
 const g=await fixture();await writeFile(join(g.root,'candidates/c001/audition/voice.json'),'changed');await expect(g.bank.save('c001',input(g))).rejects.toMatchObject({code:'conditioning_changed'});
});
test('changed listening copy, symlinks, corrupt saved records and persistent locks are not bypassed',async()=>{
 const f=await fixture();await writeFile(join(f.root,'candidates/c001/listening.m4a'),'changed');await expect(f.bank.audio('c001')).rejects.toMatchObject({code:'audio_changed'});
 const g=await fixture();await rm(join(g.root,'candidates/c001/listening.m4a'));await symlink(join(g.root,'candidates/c002/listening.m4a'),join(g.root,'candidates/c001/listening.m4a'));await expect(g.bank.audio('c001')).rejects.toMatchObject({code:'curation_path_denied'});
 const h=await fixture();await h.bank.save('c001',input(h));await writeFile(join(h.root,'curation-reviews/c001.json'),'{broken');await expect(h.bank.state()).rejects.toMatchObject({code:'curation_corrupt'});
 const k=await fixture();await k.bank.save('c001',input(k));await writeFile(join(k.root,'curation-reviews/c001.lock'),'operator inspection');await expect(k.bank.save('c001',input(k,'reject',1))).rejects.toMatchObject({code:'review_busy'});
});
test('portable bank and reviews roundtrip without checkout paths',async()=>{
 const f=await fixture();await f.bank.save('c001',input(f));const before=await f.bank.state(),copy=f.root+'-copy';roots.push(copy);await cp(f.root,copy,{recursive:true});expect(await new CurationBank(copy).state()).toEqual(before);
});
test('existing HTTP host/origin/media-range guards cover curation and reject inference-like fields',async()=>{
 const f=await fixture();const app=createApp({origin:'http://127.0.0.1:8804',data:join(f.root,'app'),info:{},publicDir:join(import.meta.dir,'public'),media:new Map(),allowedRoots:[f.root],maxDaily:1,curation:f.bank,render:async()=>{throw new Error('Must not render');}});
 const req=(path:string,options:any={})=>new Request('http://127.0.0.1:8804'+path,{...options,headers:{Host:'127.0.0.1:8804',...options.headers}});
 expect((await app(req('/api/curation',{headers:{Host:'evil.invalid'}}))).status).toBe(421);
 expect((await app(req('/api/curation/c001',{method:'POST',body:JSON.stringify(input(f))}))).status).toBe(403);
 const state=await app(req('/api/curation'));expect(state.status).toBe(200);expect((await state.json()).reviewed).toBe(0);
 const audio=await app(req('/media/curation-c001',{headers:{Range:'bytes=0-63'}}));expect(audio.status).toBe(206);expect((await audio.arrayBuffer()).byteLength).toBe(64);expect(audio.headers.get('content-type')).toBe('audio/mp4');
 expect((await app(req('/media/curation-c001',{headers:{Range:'bytes=9999999-'}}))).status).toBe(416);
 expect((await app(req('/api/curation/c001',{method:'POST',headers:{Origin:'http://127.0.0.1:8804'},body:JSON.stringify({...input(f),infer:true})}))).status).toBe(400);
 expect((await app(req('/api/curation/c001',{method:'POST',headers:{Origin:'http://127.0.0.1:8804'},body:JSON.stringify(input(f))}))).status).toBe(201);
});
test('CLI reads and explicit writes share exactly the browser state and return structured failures',async()=>{
 const f=await fixture(),file=join(f.root,'intent.json');await writeFile(file,JSON.stringify(input(f)));
 const call=async(args:string[])=>{const p=Bun.spawn(['bun',join(import.meta.dir,'curation-cli.ts'),...args],{stdout:'pipe',stderr:'pipe'});const output=await new Response(p.stdout).json();return {code:await p.exited,output};};
 expect((await call(['read','--bank',f.root])).output.reviewed).toBe(0);
 expect((await call(['save','--bank',f.root,'--candidate','c001','--input',file])).code).toBe(0);expect((await f.bank.state()).reviewed).toBe(1);
 const bad=await call(['save','--bank',f.root]);expect(bad.code).toBe(1);expect(bad.output.error).toBe('invalid_command');
});
