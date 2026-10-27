import { test,expect,afterEach } from 'bun:test';
import { readFile,writeFile,rm } from 'node:fs/promises';
import { join } from 'node:path';
import { randomUUID,createHash } from 'node:crypto';
import { curationFixture } from './curation-fixture';
import { addDesignFixture } from './curation-design-fixture';
import { CurationBank } from './curation';
import { CurationCatalog } from './curation-catalog';
import { createApp } from './core';
const roots:string[]=[];afterEach(async()=>{await Promise.all(roots.splice(0).map(r=>rm(r,{recursive:true,force:true})));});
async function setup(){
 const a=await curationFixture(),b=await curationFixture();roots.push(a.root,b.root);const design=await addDesignFixture(b.root),primary=new CurationBank(a.root),characters=new CurationBank(b.root),catalog=new CurationCatalog(primary,new Map([['characters',characters]]));
 return {a,b,design,primary,characters,catalog};
}
function intent(bank:string,row:any,hash=row.audio_sha256){return {bank,audio_sha256:hash,expected_revision:0,operation_id:randomUUID(),decision:'keep',notes:''};}
test('catalog keys select real independent banks; unknown paths never fall back',async()=>{
 const f=await setup(),a=await f.catalog.state(),b=await f.catalog.state('characters');
 expect(a.selected_bank).toBe('default');expect(a.banks).toHaveLength(2);expect(b.bank).toBe('synthetic-character-fixture');expect(b.candidates[0].source_media).toBe('/media/curation-source-c001?bank=characters');
 expect(a.candidates[0].media).toBe('/media/curation-c001?bank=default');
 await expect(f.catalog.state('../../private')).rejects.toMatchObject({code:'bank_not_found'});
 expect(()=>new CurationCatalog(f.primary,new Map([['default',f.characters]]))).toThrow();
});
test('exact source prompts are receipt-bound and cannot silently relabel another source',async()=>{
 const f=await setup(),b=await f.characters.state();expect(b.candidates[0].design_prompt).toBe('Original adult clockwork c001. <em>Literal prompt characters, not markup.</em>');
 expect((await f.primary.state()).candidates[0].design_prompt).toBeUndefined();
 const file=join(f.b.root,'candidates/c001/design.json'),v=JSON.parse(await readFile(file,'utf8'));v.result.description='Changed';await writeFile(file,JSON.stringify(v));
 await expect(f.characters.state()).rejects.toMatchObject({code:'source_changed'});
 v.output_sha256='0'.repeat(64);const bytes=JSON.stringify(v);await writeFile(file,bytes);f.design.ready[0].design_receipt_sha256=createHash('sha256').update(bytes).digest('hex');await writeFile(join(f.b.root,'index.json'),JSON.stringify(f.design));
 await expect(f.characters.state()).rejects.toMatchObject({code:'source_prompt_invalid'});
});
test('same candidate ids cannot cross-save, and source audio cannot masquerade as editable audio',async()=>{
 const f=await setup(),a=await f.primary.state(),b=await f.characters.state();
 await f.primary.save('c001',intent(a.bank,a.candidates[0]));const before=await readFile(join(f.a.root,'curation-reviews/c001.json'),'utf8');
 await expect(f.characters.save('c001',intent(a.bank,a.candidates[0]))).rejects.toMatchObject({code:'audio_changed'});
 await expect(f.characters.save('c001',intent(b.bank,b.candidates[0],f.design.ready[0].design_audio_sha256))).rejects.toMatchObject({code:'audio_changed'});
 await f.characters.save('c001',intent(b.bank,b.candidates[0]));expect(await readFile(join(f.a.root,'curation-reviews/c001.json'),'utf8')).toBe(before);
 expect((await f.primary.state()).reviewed).toBe(1);expect((await f.characters.state()).reviewed).toBe(1);
});
test('source paths/bytes are hash checked without accepting source-review fields',async()=>{
 const f=await setup();expect(await f.characters.sourceAudio('c001')).toEndWith('/c001/design-listening.m4a');
 await expect(f.primary.sourceAudio('c001')).rejects.toMatchObject({code:'not_found'});
 await writeFile(join(f.b.root,'candidates/c001/reference.wav'),'changed');await expect(f.characters.sourceAudio('c001')).rejects.toMatchObject({code:'source_changed'});
 const b=await f.characters.state();await expect(f.characters.save('c001',{...intent(b.bank,b.candidates[0]),variant:'source'})).rejects.toMatchObject({code:'invalid_review'});
});
test('HTTP bank queries scope state, media, source ranges and writes; default remains compatible',async()=>{
 const f=await setup(),origin='http://127.0.0.1:8804';
 const app=createApp({origin,data:join(f.a.root,'app'),info:{},publicDir:join(import.meta.dir,'public'),media:new Map(),allowedRoots:[f.a.root,f.b.root],maxDaily:1,curationCatalog:f.catalog,render:async()=>{throw Error('No render');}});
 const request=(path:string,body?:any)=>new Request(origin+path,{method:body?'POST':'GET',headers:{Host:'127.0.0.1:8804',Origin:origin},body:body?JSON.stringify(body):undefined});
 expect((await app(request('/api/curation?bank=missing'))).status).toBe(404);
 const state=await(await app(request('/api/curation?bank=characters'))).json();expect(state.selected_bank).toBe('characters');
 const media=await app(new Request(origin+state.candidates[0].source_media,{headers:{Host:'127.0.0.1:8804',Range:'bytes=0-63'}}));expect(media.status).toBe(206);expect((await media.arrayBuffer()).byteLength).toBe(64);
 expect((await app(request('/api/curation/c001?bank=characters',intent('synthetic-curation-fixture',f.a.rows[0])))).status).toBe(409);
 expect((await app(request('/api/curation/c001?bank=characters',intent(state.bank,state.candidates[0])))).status).toBe(201);
 expect((await(await app(request('/api/curation'))).json()).reviewed).toBe(0);
});
