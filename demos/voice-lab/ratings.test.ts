import { expect, test, afterAll } from 'bun:test';
import { randomUUID } from 'node:crypto';
import { mkdtemp, mkdir, readFile, readdir, rename, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { RatingBank, traits, validateRating } from './ratings';
import { ratingFixture } from './ratings-fixture';
import { createApp } from './core';
const roots:string[]=[];
afterAll(async()=>{await Promise.all(roots.map(root=>rm(root,{recursive:true,force:true})));});
async function setup(){
 const root=await mkdtemp(join(tmpdir(),'spqx-ratings-'));roots.push(root);const fixture=await ratingFixture(root);const bank=new RatingBank(root);
 const value={bank:'synthetic-test-bank',audio_sha256:fixture.ready[0].audio_sha256,operation_id:randomUUID(),expected_revision:0,scores:Object.fromEntries(traits.map(t=>[t.id,50])),naturalness:70,excluded:false,pronunciation_issue:false,notes:'Synthetic test annotation'};
 let renders=0;
 const app=createApp({origin:'http://127.0.0.1:8805',data:join(root,'app'),info:{},publicDir:join(import.meta.dir,'public'),media:new Map(),allowedRoots:[root],maxDaily:1,ratings:bank,render:async()=>{renders++;throw new Error('must not render');}});
 const req=(path:string,body?:unknown,origin:string|null='http://127.0.0.1:8805')=>new Request('http://127.0.0.1:8805'+path,{method:body?'POST':'GET',headers:{Host:'127.0.0.1:8805',...(body?{'Content-Type':'application/json',...(origin?{Origin:origin}:{})}:{})},body:body?JSON.stringify(body):undefined});
 return {root,bank,value,app,req,renders:()=>renders};
}
test('blind unrated bank whitelists state and never creates midpoint labels',async()=>{
 const {root,bank}=await setup(),state=await bank.state();expect(state.rated).toBe(0);expect(state.candidates.every(c=>c.rating===null)).toBe(true);expect(JSON.stringify(state)).not.toContain('HIDDEN');expect(state.traits).toHaveLength(6);expect(await readdir(root)).not.toContain('ratings');
});
test('rating bodies have exact six finite integer traits and separate quality',async()=>{
 const {value}=await setup();expect(validateRating(value)).toEqual(value);
 for(const bad of [{...value,command:'exec'},{...value,naturalness:null},{...value,scores:{...value.scores,pitch:101}},{...value,scores:{...value.scores,pitch:1.1}},{...value,scores:{...value.scores,age:50}},{...value,scores:{}},{...value,notes:'x'.repeat(601)},{...value,excluded:'yes'},{...value,operation_id:'../../outside'}])expect(()=>validateRating(bad)).toThrow();
});
test('exact audition-bound save persists across reload; ack-lost retry is idempotent',async()=>{
 const {root,bank,value}=await setup();const first=await bank.save('c001',value);expect(first.revision).toBe(1);expect(first.source).toBe('explicit-rating');expect(await new RatingBank(root).save('c001',Object.fromEntries(Object.entries(value).reverse()))).toEqual(first);
 expect((await bank.state()).rated).toBe(1);expect((await bank.state()).candidates[0].rating?.scores).toEqual(value.scores);expect((await readdir(join(root,'ratings')))).toEqual(['c001.json']);
 await expect(bank.save('c001',{...value,notes:'changed'})).rejects.toMatchObject({code:'operation_conflict'});
});
test('stale tabs and changed audio/bank fail without overwriting scores',async()=>{
 const {root,bank,value}=await setup();await bank.save('c001',value);const before=await readFile(join(root,'ratings/c001.json'));
 for(const change of [{operation_id:randomUUID()},{bank:'old-bank'},{audio_sha256:'f'.repeat(64)}])await expect(bank.save('c001',{...value,...change})).rejects.toMatchObject({status:409});
 expect(await readFile(join(root,'ratings/c001.json'))).toEqual(before);
 const second=await bank.save('c001',{...value,operation_id:randomUUID(),expected_revision:1,naturalness:80});expect(second.revision).toBe(2);
});
test('concurrent independent stores serialize; one stale writer loses',async()=>{
 const {root,bank,value}=await setup();const result=await Promise.allSettled([bank.save('c001',value),new RatingBank(root).save('c001',{...value,operation_id:randomUUID(),naturalness:20})]);
 expect(result.filter(r=>r.status==='fulfilled')).toHaveLength(1);expect((await bank.state()).rated).toBe(1);expect(await readdir(join(root,'ratings'))).toEqual(['c001.json']);
});
test('changed compressed or raw audition fails closed with zero ratings',async()=>{
 for(const file of ['listening.m4a','audition/reference.wav']){
  const {root,bank,value}=await setup();await writeFile(join(root,'candidates/c001',file),'changed');await expect(bank.save('c001',value)).rejects.toMatchObject({code:'audio_changed'});expect((await bank.state()).rated).toBe(0);
 }
});
test('corrupt score records are not treated as missing or silently replaced',async()=>{
 const {root,bank,value}=await setup();await bank.save('c001',value);const p=join(root,'ratings/c001.json'),v=JSON.parse(await readFile(p,'utf8'));v.scores.pitch=20;await writeFile(p,JSON.stringify(v));
 await expect(bank.state()).rejects.toMatchObject({code:'rating_corrupt'});await expect(bank.save('c001',{...value,expected_revision:1,operation_id:randomUUID()})).rejects.toMatchObject({code:'rating_corrupt'});
});
test('candidate, ratings and media symlink escapes are denied',async()=>{
 const {root,bank,value}=await setup();const outside=await mkdtemp(join(tmpdir(),'spqx-rating-outside-'));roots.push(outside);
 await writeFile(join(outside,'media'),'outside');await rm(join(root,'candidates/c001/listening.m4a'));await symlink(join(outside,'media'),join(root,'candidates/c001/listening.m4a'));await expect(bank.audio('c001')).rejects.toMatchObject({code:'rating_path_denied'});
 const another=await setup();await symlink(outside,join(another.root,'ratings'));await expect(another.bank.save('c002',{...another.value,audio_sha256:another.value.audio_sha256})).rejects.toMatchObject({code:'rating_path_denied'});
 await expect(bank.audio('../outside')).rejects.toMatchObject({code:'not_found'});
});
test('existing Host/Origin guard covers ratings; reads/saves spend no inference budget',async()=>{
 const {root,app,req,value,renders}=await setup();expect((await app(req('/api/ratings'))).status).toBe(200);
 for(const origin of [null,'null','https://attacker.invalid'])expect((await app(req('/api/ratings/c001',value,origin))).status).toBe(403);
 const hostile=req('/api/ratings');hostile.headers.set('Host','attacker.invalid');expect((await app(hostile)).status).toBe(421);
 expect((await app(req('/api/ratings/c001',value))).status).toBe(201);expect(renders()).toBe(0);expect(await readdir(join(root,'app'))).not.toContain('budget.json');
 const range=req('/media/rating-c001');range.headers.set('Range','bytes=0-7');const response=await app(range);expect(response.status).toBe(206);expect(response.headers.get('Content-Type')).toBe('audio/mp4');expect((await response.arrayBuffer()).byteLength).toBe(8);
});
