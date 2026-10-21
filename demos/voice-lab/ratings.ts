import { createHash, randomUUID } from 'node:crypto';
import { mkdir, open, readFile, realpath, rename, unlink, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

export const traits = [
  {id:'pitch',label:'Pitch',low:'Low',high:'High'},
  {id:'brightness',label:'Brightness',low:'Dark',high:'Bright'},
  {id:'breathiness',label:'Breathiness',low:'Clear',high:'Airy'},
  {id:'roughness',label:'Roughness',low:'Smooth',high:'Raspy'},
  {id:'weight',label:'Vocal weight',low:'Light / thin',high:'Full / heavy'},
  {id:'nasality',label:'Nasality',low:'Open / oral',high:'Nasal'},
] as const;
// Categories are discrete judgements, not continua: there is no meaningful midpoint
// default, so each must be chosen explicitly. They never leak the hidden requested description.
export const categories = [
  {id:'gender',label:'Voice presentation',options:[{id:'masculine',label:'Masculine'},{id:'feminine',label:'Feminine'},{id:'androgynous',label:'Androgynous'},{id:'unclear',label:'Unclear'}]},
  {id:'age_band',label:'Age band',options:[{id:'young',label:'Young adult'},{id:'middle',label:'Middle-aged adult'},{id:'older',label:'Older adult'},{id:'unclear',label:'Unclear'}]},
] as const;
export type CategoryId=(typeof categories)[number]['id'];
export type RatingChoice=(typeof categories)[number]['options'][number]['id']|null;
export class RatingError extends Error {
  constructor(public code:string,message:string,public status=400){super(message);}
}
const hash=(v:string|Uint8Array)=>createHash('sha256').update(v).digest('hex');
const sha=(v:unknown):v is string=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const cid=(v:unknown):v is string=>typeof v==='string'&&/^c\d{3}$/.test(v);
const score=(v:unknown):v is number=>typeof v==='number'&&Number.isInteger(v)&&v>=0&&v<=100;
const plain=(v:any)=>v!==null&&typeof v==='object'&&!Array.isArray(v);
export type Candidate={id:string;seconds:number;audio_sha256:string;listening_sha256:string};
export type RatingInput={bank:string;audio_sha256:string;operation_id:string;expected_revision:number;scores:Record<string,number>;naturalness:number;excluded:boolean;pronunciation_issue:boolean;notes:string}&Record<CategoryId,RatingChoice>;
export type Rating=RatingInput&{version:1;candidate:string;revision:number;updated:string;source:'explicit-rating';fingerprint:string};
export function validateRating(v:any):RatingInput {
  const keys=[...categories.map(c=>c.id),'audio_sha256','bank','excluded','expected_revision','naturalness','notes','operation_id','pronunciation_issue','scores'].sort().join(',');
  if(!plain(v)||Object.keys(v).sort().join(',')!==keys||typeof v.bank!=='string'||v.bank.length>128||!sha(v.audio_sha256)||typeof v.operation_id!=='string'||!/^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/.test(v.operation_id)||!Number.isSafeInteger(v.expected_revision)||v.expected_revision<0||!score(v.naturalness)||typeof v.excluded!=='boolean'||typeof v.pronunciation_issue!=='boolean'||typeof v.notes!=='string'||v.notes.length>600||/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(v.notes))
    throw new RatingError('invalid_rating','Send all six scores, both categories, naturalness, flags, notes and the displayed audio revision.');
  if(!plain(v.scores)||Object.keys(v.scores).sort().join(',')!==traits.map(t=>t.id).sort().join(',')||traits.some(t=>!score(v.scores[t.id])))throw new RatingError('invalid_scores','Each trait is an integer from0 to100.');
  const chosen=Object.fromEntries(categories.map(c=>[c.id,v[c.id]===null?null:(c.options.some(o=>o.id===v[c.id])?v[c.id]:undefined)]));
  if(categories.some(c=>chosen[c.id]!==null&&chosen[c.id]===undefined))throw new RatingError('invalid_categories','Choose one listed option per category, or null for unrated.');
  return {...chosen,bank:v.bank,audio_sha256:v.audio_sha256,operation_id:v.operation_id,expected_revision:v.expected_revision,scores:Object.fromEntries(traits.map(t=>[t.id,v.scores[t.id]])),naturalness:v.naturalness,excluded:v.excluded,pronunciation_issue:v.pronunciation_issue,notes:v.notes.normalize('NFC')};
}

/** Private file-backed annotations. No synthesis, trait inference or registry operations. */
export class RatingBank {
  private root:Promise<string>;
  constructor(directory:string){this.root=realpath(resolve(directory));}
  private async path(...parts:string[]){return join(await this.root,...parts);}
  private async confined(...parts:string[]){const p=await this.path(...parts);if(await realpath(p)!==p)throw new RatingError('rating_path_denied','A rating asset uses an unsupported symlink.',403);return p;}
  private async index(){
    const p=await this.confined('index.json');const bytes=await readFile(p,'utf8');
    if(bytes.length>300000)throw new RatingError('bank_invalid','The audition bank is too large.',503);
    const v=JSON.parse(bytes);
    if(v.version!==1||v.mode!=='vector_only'||typeof v.id!=='string'||v.id.length>128||typeof v.text!=='string'||v.text.length>2000||!Number.isInteger(v.target_count)||v.target_count<1||v.target_count>200||!Array.isArray(v.ready)||v.ready.length>200)throw new RatingError('bank_invalid','The vector-only audition bank is incompatible.',503);
    if(v.traits!==undefined&&JSON.stringify(v.traits)!==JSON.stringify(traits))throw new RatingError('bank_invalid','Bank trait definitions do not match the rater.',503);
    if(v.categories!==undefined&&JSON.stringify(v.categories)!==JSON.stringify(categories))throw new RatingError('bank_invalid','Bank category definitions do not match the rater.',503);
    const ready:Candidate[]=v.ready.map((c:any)=>{
      if(!cid(c.id)||!sha(c.audio_sha256)||!sha(c.listening_sha256)||!Number.isFinite(c.seconds)||c.seconds<2||c.seconds>20)throw new RatingError('bank_invalid','An audition record is invalid.',503);
      return {id:c.id,seconds:c.seconds,audio_sha256:c.audio_sha256,listening_sha256:c.listening_sha256};
    });
    if(new Set(ready.map(c=>c.id)).size!==ready.length)throw new RatingError('bank_invalid','Duplicate audition ids.',503);
    return {id:v.id,text:v.text,target_count:v.target_count,status:String(v.status),ready};
  }
  private async current(id:string):Promise<Rating|null>{
    try{
      const bytes=await readFile(await this.confined('ratings',id+'.json'),'utf8');
      if(bytes.length>5000)throw new Error('oversize');
      const v=JSON.parse(bytes),input=validateRating(Object.fromEntries(Object.entries(v).filter(([k])=>!['version','candidate','revision','updated','source','fingerprint'].includes(k))));
      if(v.version!==1||v.candidate!==id||!Number.isSafeInteger(v.revision)||v.revision!==v.expected_revision+1||v.source!=='explicit-rating'||v.fingerprint!==hash(JSON.stringify(input))||typeof v.updated!=='string'||!Number.isFinite(Date.parse(v.updated)))throw new Error('format');
      return v;
    }catch(error:any){if(error.code==='ENOENT')return null;if(error instanceof RatingError&&error.code==='rating_path_denied')throw error;throw new RatingError('rating_corrupt','A saved rating needs operator inspection. It was not replaced.',503);}
  }
  async state(){
    const bank=await this.index();
    const candidates=await Promise.all(bank.ready.map(async c=>{
      const rating=await this.current(c.id);
      if(rating&&(rating.bank!==bank.id||rating.audio_sha256!==c.audio_sha256))throw new RatingError('rating_stale','A saved rating belongs to different audio. Ask the operator.',409);
      return {...c,media:'/media/rating-'+c.id,rating};
    }));
    return {version:1,bank:bank.id,text:bank.text,target_count:bank.target_count,generation_status:bank.status,traits,categories,quality:{label:'Naturalness',low:'Broken / unusable',high:'Natural / effortless'},candidates,rated:candidates.filter(c=>c.rating).length};
  }
  async audio(id:string){
    if(!cid(id))throw new RatingError('not_found','No such audition.',404);
    const candidate=(await this.index()).ready.find(c=>c.id===id);
    if(!candidate)throw new RatingError('not_found','This audition is not ready.',404);
    const path=await this.confined('candidates',id,'listening.m4a');
    if(hash(await readFile(path))!==candidate.listening_sha256)throw new RatingError('audio_changed','The listening copy changed. It was not served.',409);
    return path;
  }
  async save(id:string,input:unknown){
    if(!cid(id))throw new RatingError('not_found','No such audition.',404);
    const value=validateRating(input),bank=await this.index(),candidate=bank.ready.find(c=>c.id===id);
    if(!candidate)throw new RatingError('not_found','This audition is not ready.',404);
    if(value.bank!==bank.id||value.audio_sha256!==candidate.audio_sha256)throw new RatingError('audio_changed','The bank or audition changed. Reload before rating.',409);
    await this.audio(id);
    const raw=await this.confined('candidates',id,'audition','reference.wav');
    if(hash(await readFile(raw))!==candidate.audio_sha256)throw new RatingError('audio_changed','The raw audition changed. No rating was written.',409);
    const directory=await this.path('ratings');await mkdir(directory,{recursive:true,mode:0o700});await this.confined('ratings');
    const lockPath=join(directory,id+'.lock');let lock;
    try{lock=await open(lockPath,'wx',0o600);}catch(error:any){if(error.code==='EEXIST')throw new RatingError('rating_busy','Another save is finishing. Retry shortly; persistent locks need operator inspection.',409);throw error;}
    let temporary:string|undefined;
    try{
      await lock.writeFile(JSON.stringify({pid:process.pid,operation_id:value.operation_id,created:new Date().toISOString()}));
      const current=await this.current(id),fingerprint=hash(JSON.stringify(value));
      if(current?.operation_id===value.operation_id){if(current.fingerprint!==fingerprint)throw new RatingError('operation_conflict','This save intent was already used for different scores.',409);return current;}
      if((current?.revision||0)!==value.expected_revision)throw new RatingError('revision_conflict','This rating changed in another tab. Reload before saving.',409);
      const rating:Rating={...value,version:1,candidate:id,revision:value.expected_revision+1,updated:new Date().toISOString(),source:'explicit-rating',fingerprint};
      temporary=join(directory,id+'.'+randomUUID()+'.tmp');
      await writeFile(temporary,JSON.stringify(rating,null,2)+'\n',{flag:'wx',mode:0o600});await rename(temporary,join(directory,id+'.json'));temporary=undefined;
      return rating;
    }finally{await lock.close();await unlink(lockPath);if(temporary)await unlink(temporary).catch(()=>{});}
  }
}
