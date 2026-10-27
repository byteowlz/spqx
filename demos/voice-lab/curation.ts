import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readFile, realpath, open, rename, unlink, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

export class CurationError extends Error {
  constructor(public code:string,message:string,public status=400){super(message);}
}
export type ReviewInput={bank:string;audio_sha256:string;expected_revision:number;operation_id:string;decision:'keep'|'reject'|'unsure'|null;notes:string};
export type Review=ReviewInput&{version:1;candidate:string;revision:number;updated:string;fingerprint:string};
const hash=(value:Uint8Array|string)=>createHash('sha256').update(value).digest('hex');
const sha=(v:unknown):v is string=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const cid=(v:unknown):v is string=>typeof v==='string'&&/^c\d{3}$/.test(v);
const plain=(v:any)=>v!==null&&typeof v==='object'&&!Array.isArray(v);
export function validateReview(v:any):ReviewInput {
  if(!plain(v)||Object.keys(v).sort().join(',')!=='audio_sha256,bank,decision,expected_revision,notes,operation_id'||typeof v.bank!=='string'||v.bank.length<1||v.bank.length>128||!sha(v.audio_sha256)||!Number.isSafeInteger(v.expected_revision)||v.expected_revision<0||v.expected_revision===Number.MAX_SAFE_INTEGER||typeof v.operation_id!=='string'||!/^[a-f0-9-]{36}$/.test(v.operation_id)||![null,'keep','reject','unsure'].includes(v.decision)||typeof v.notes!=='string'||v.notes.length>600||/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(v.notes))
    throw new CurationError('invalid_review','Send an explicit keep, reject, unsure or null decision, optional notes and exact bank/audio/revision/save-intent fields.');
  return {bank:v.bank,audio_sha256:v.audio_sha256,expected_revision:v.expected_revision,operation_id:v.operation_id,decision:v.decision,notes:v.notes};
}

function sourceFields(c:any){
  const present=c.design_audio_sha256!==undefined||c.design_listening_sha256!==undefined||c.design_seconds!==undefined;
  if(c.design_receipt_sha256!==undefined&&(!present||!sha(c.design_receipt_sha256)))throw new CurationError('curation_bank_invalid','The design prompt lacks receipt custody.',503);
  if(!present)return {};
  if(!sha(c.design_audio_sha256)||!sha(c.design_listening_sha256)||!Number.isFinite(c.design_seconds)||c.design_seconds<2||c.design_seconds>20)throw new CurationError('curation_bank_invalid','The design comparison lacks exact source custody.',503);
  return {design_audio_sha256:c.design_audio_sha256,design_listening_sha256:c.design_listening_sha256,design_seconds:c.design_seconds,...(c.design_receipt_sha256?{design_receipt_sha256:c.design_receipt_sha256}:{})};
}

/** One private substrate for browser/API/CLI curation; never renders or enrolls. */
export class CurationBank {
  private root:Promise<string>;
  constructor(directory:string){this.root=realpath(resolve(directory));}
  private async path(...parts:string[]){return join(await this.root,...parts);}
  private async confined(...parts:string[]){const p=await this.path(...parts);if(await realpath(p)!==p)throw new CurationError('curation_path_denied','An audition or review uses an unsupported symlink.',403);return p;}
  private async index(){
    const bytes=await readFile(await this.confined('index.json'),'utf8');
    if(bytes.length>300000)throw new CurationError('curation_bank_invalid','The audition index is too large.',503);
    const v=JSON.parse(bytes);
    if(!plain(v)||v.version!==1||typeof v.id!=='string'||v.id.length<1||v.id.length>128||typeof v.text!=='string'||v.text.length>2000||!Array.isArray(v.ready)||v.ready.length<1||v.ready.length>200)throw new CurationError('curation_bank_invalid','No compatible completed audition bank was configured.',503);
    const ready=v.ready.map((c:any)=>{
      if(!plain(c)||!cid(c.id)||!sha(c.audio_sha256)||!sha(c.listening_sha256)||!sha(c.vector_f32_sha256)||!sha(c.conditioning_sha256)||!Number.isFinite(c.seconds)||c.seconds<2||c.seconds>20)throw new CurationError('curation_bank_invalid','An audition lacks exact audio/vector custody.',503);
      return {id:c.id,seconds:c.seconds,audio_sha256:c.audio_sha256,listening_sha256:c.listening_sha256,vector_f32_sha256:c.vector_f32_sha256,conditioning_sha256:c.conditioning_sha256,...sourceFields(c)};
    });
    if(new Set(ready.map((c:any)=>c.id)).size!==ready.length)throw new CurationError('curation_bank_invalid','Duplicate audition IDs.',503);
    return {id:v.id,title:typeof v.title==='string'&&v.title.length<=160?v.title:'Voice exploration',text:v.text,ready};
  }
  private async current(id:string):Promise<Review|null>{
    try {
      const bytes=await readFile(await this.confined('curation-reviews',id+'.json'),'utf8');
      if(bytes.length>6000)throw new Error('oversize');
      const v=JSON.parse(bytes),input=validateReview(Object.fromEntries(['bank','audio_sha256','expected_revision','operation_id','decision','notes'].map(k=>[k,v[k]])));
      if(v.version!==1||v.candidate!==id||v.revision!==input.expected_revision+1||typeof v.updated!=='string'||v.fingerprint!==hash(JSON.stringify(input)))throw new Error('format');
      return v;
    } catch(error:any){if(error.code==='ENOENT')return null;if(error instanceof CurationError&&error.code==='curation_path_denied')throw error;throw new CurationError('curation_corrupt','A saved review needs operator inspection; it was not replaced.',503);}
  }
  async summary(){const bank=await this.index();return {bank:bank.id,title:bank.title,count:bank.ready.length};}
  async state(){
    const bank=await this.index();
    const candidates=await Promise.all(bank.ready.map(async(c:any)=>{
      const review=await this.current(c.id);
      if(review&&(review.bank!==bank.id||review.audio_sha256!==c.audio_sha256))throw new CurationError('curation_stale','A saved review belongs to different audio; it was not relabelled.',409);
      const design_prompt=await this.sourcePrompt(c,bank.text);
      return {...c,media:'/media/curation-'+c.id,...(c.design_audio_sha256?{source_media:'/media/curation-source-'+c.id}:{}),...(design_prompt?{design_prompt}:{}),review};
    }));
    return {version:1,bank:bank.id,title:bank.title,text:bank.text,review_target:'editable',candidates,reviewed:candidates.filter((c:any)=>c.review?.decision!=null).length};
  }
  private async sourcePrompt(c:any,text:string){
    if(!c.design_receipt_sha256)return;
    const bytes=await readFile(await this.confined('candidates',c.id,'design.json'));
    if(bytes.length>32000||hash(bytes)!==c.design_receipt_sha256)throw new CurationError('source_changed','The design prompt receipt changed; it was not shown.',409);
    const receipt=JSON.parse(bytes.toString()),r=receipt.result;
    if(receipt.output_sha256!==c.design_audio_sha256||r?.operation!=='voice_design'||r.eos_reached!==true||r.text!==text||typeof r.description!=='string'||r.description.length<1||r.description.length>6000)throw new CurationError('source_prompt_invalid','The prompt receipt does not match this completed source audition.',503);
    return r.description as string;
  }
  private async target(id:string){
    if(!cid(id))throw new CurationError('not_found','No such audition.',404);
    const bank=await this.index(),c=bank.ready.find((c:any)=>c.id===id);
    if(!c)throw new CurationError('not_found','No such completed audition.',404);
    return {bank,c};
  }
  private async verifiedAsset(id:string,asset:{file:string;sha256:string;code:string}){
    const path=await this.confined('candidates',id,asset.file);
    if(hash(await readFile(path))!==asset.sha256)throw new CurationError(asset.code,'This audio changed; it was not served.',409);
    return path;
  }
  async audio(id:string){
    const {c}=await this.target(id);
    return this.verifiedAsset(id,{file:'listening.m4a',sha256:c.listening_sha256,code:'audio_changed'});
  }
  async sourceAudio(id:string){
    const {c}=await this.target(id);
    if(!c.design_audio_sha256)throw new CurationError('not_found','No original design comparison is available.',404);
    await this.verifiedAsset(id,{file:'reference.wav',sha256:c.design_audio_sha256,code:'source_changed'});
    return this.verifiedAsset(id,{file:'design-listening.m4a',sha256:c.design_listening_sha256,code:'source_changed'});
  }
  private async checkFrozen(id:string,c:any,text:string){
    if(hash(await readFile(await this.confined('candidates',id,'audition','reference.wav')))!==c.audio_sha256)throw new CurationError('audio_changed','The raw audition changed; no review was saved.',409);
    const bytes=await readFile(await this.confined('candidates',id,'audition','voice.json'));
    if(hash(bytes)!==c.conditioning_sha256)throw new CurationError('conditioning_changed','The frozen audition changed; no review was saved.',409);
    const v=JSON.parse(bytes.toString()),values=v.conditioning?.values;
    if(v.version!==1||v.text!==text||v.reference_sha256!==c.audio_sha256||!Array.isArray(values)||values.length!==1024||values.some((n:unknown)=>typeof n!=='number'||!Number.isFinite(n)))throw new CurationError('conditioning_changed','The frozen conditioning does not match this audition.',409);
    const raw=Buffer.alloc(4096);values.forEach((n:number,i:number)=>raw.writeFloatLE(n,i*4));
    if(hash(raw)!==c.vector_f32_sha256)throw new CurationError('conditioning_changed','The effective vector changed; no review was saved.',409);
  }
  async save(id:string,unknown:unknown){
    const input=validateReview(unknown),{bank,c}=await this.target(id);
    if(input.bank!==bank.id||input.audio_sha256!==c.audio_sha256)throw new CurationError('audio_changed','This save targets different bank/audio; reload before saving.',409);
    await this.audio(id);await this.checkFrozen(id,c,bank.text);
    const directory=await this.path('curation-reviews');await mkdir(directory,{recursive:true,mode:0o700});await this.confined('curation-reviews');
    const lockPath=join(directory,id+'.lock');let lock;
    try{lock=await open(lockPath,'wx',0o600);}catch(error:any){if(error.code==='EEXIST')throw new CurationError('review_busy','Another save is finishing. Retry shortly; persistent locks need operator inspection.',409);throw error;}
    let temporary:string|undefined;
    try{
      await lock.writeFile(JSON.stringify({pid:process.pid,operation_id:input.operation_id,created:new Date().toISOString()}));
      const current=await this.current(id),fingerprint=hash(JSON.stringify(input));
      if(current&&(current.bank!==bank.id||current.audio_sha256!==c.audio_sha256))throw new CurationError('curation_stale','The saved review belongs to different bank/audio; it was not replaced.',409);
      if(current?.operation_id===input.operation_id){if(current.fingerprint!==fingerprint)throw new CurationError('operation_conflict','This save intent already has different content.',409);return current;}
      if((current?.revision||0)!==input.expected_revision)throw new CurationError('revision_conflict','This review changed elsewhere. Your draft is kept; reload saved review before replacing it.',409);
      if(current){const backup=join(directory,id+'.previous.'+randomUUID()+'.tmp');await writeFile(backup,JSON.stringify(current,null,2)+'\n',{flag:'wx',mode:0o600});await rename(backup,join(directory,id+'.previous.json'));}
      const review:Review={...input,version:1,candidate:id,revision:input.expected_revision+1,updated:new Date().toISOString(),fingerprint};
      temporary=join(directory,id+'.'+randomUUID()+'.tmp');const file=await open(temporary,'wx',0o600);
      try{await file.writeFile(JSON.stringify(review,null,2)+'\n');await file.sync();}finally{await file.close();}
      await rename(temporary,join(directory,id+'.json'));temporary=undefined;return review;
    }finally{await lock.close();await unlink(lockPath);if(temporary)await unlink(temporary);}
  }
}
