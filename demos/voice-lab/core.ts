import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readFile, writeFile, rename, realpath, readdir } from 'node:fs/promises';
import { join, resolve, sep } from 'node:path';
import { captureConditioning } from './frozen';
import { RatingError, type RatingBank } from './ratings';

export class LabError extends Error {
  constructor(public code: string, message: string, public status = 400) { super(message); }
}
export type Recipe = { base: string; axes: number[]; space: string };
export type Result = { wave: Uint8Array; seconds: number; model: string; basis: string; native_voice_json?:string; sdk_conditioning_json?:string };
export type Job = { id: string; status: string; recipe: Recipe; created: string; error?: string; message?: string; seconds?: number; sha256?: string; frozen?:{kind:'native'|'sdk';sha256:string} };
export const digest = (v: Uint8Array | string) => createHash('sha256').update(v).digest('hex');
export function validateRecipe(value: any, space: string): Recipe {
  if (!value || Object.keys(value).sort().join(',') !== 'axes,base,space' || !['A','C','D'].includes(value.base) || value.space !== space || !Array.isArray(value.axes) || value.axes.length !== 2 || value.axes.some((n: unknown) => typeof n !== 'number' || !Number.isFinite(n) || Math.abs(n) > .75))
    throw new LabError('invalid_recipe', 'Choose a source and two finite values between -0.75 and +0.75 in this space.');
  return { base: value.base, axes: [...value.axes], space };
}
export function validWave(bytes: Uint8Array, seconds: number) {
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
  if(bytes.length<44 || Buffer.from(bytes.subarray(0,4)).toString()!=='RIFF' || Buffer.from(bytes.subarray(8,12)).toString()!=='WAVE')return false;
  let format=false,data=false;
  for(let offset=12;offset+8<=bytes.length;){
    const name=Buffer.from(bytes.subarray(offset,offset+4)).toString(),length=view.getUint32(offset+4,true),start=offset+8;
    if(start+length>bytes.length)return false;
    if(name==='fmt '){
      const encoding=length>=16?view.getUint16(start,true):0;
      const extensible=encoding===65534&&length>=40&&view.getUint16(start+16,true)>=22&&view.getUint16(start+18,true)===32&&Buffer.from(bytes.subarray(start+24,start+40)).toString('hex')==='0300000000001000800000aa00389b71';
      format=length>=16&&(encoding===3||extensible)&&view.getUint16(start+2,true)===1&&view.getUint32(start+4,true)===24000&&view.getUint32(start+8,true)===96000&&view.getUint16(start+12,true)===4&&view.getUint16(start+14,true)===32;
    }
    if(name==='data'){
      if(length%4 || Math.abs(length/96000-seconds)>.001)return false;
      let energy=0;
      for(let i=start;i<start+length;i+=4){const n=view.getFloat32(i,true);if(!Number.isFinite(n)||Math.abs(n)>=.99999)return false;energy+=n*n;}
      data=length>0 && Math.sqrt(energy/(length/4))>1e-5;
    }
    offset=start+length+(length%2);
  }
  return format&&data;
}
async function atomic(path: string, value: unknown) {
  const temporary = path + '.' + randomUUID() + '.tmp';
  await writeFile(temporary, JSON.stringify(value, null, 2), { mode: 0o600 });
  await rename(temporary, path);
}
export function createApp(options: {
  origin: string; data: string; info: any; publicDir: string;
  media: Map<string,string>; allowedRoots: string[]; maxDaily: number; recipeFiles?: Record<string,string>; ratings?: RatingBank;
  render: (recipe: Recipe, id: string, admitted: () => Promise<void>) => Promise<Result>;
  promote?: (directory:string,name:string,operationId:string)=>Promise<string>;
}) {
  const jobs = new Map<string,Job>();
  let active: string | null = null;
  let saving = false;
  let budget = { day: '', used: 0 };
  let allowedRoots: string[] = [];
  const ready = (async () => {
    await mkdir(join(options.data,'jobs'), { recursive:true, mode:0o700 });
    await mkdir(join(options.data,'drafts'), { recursive:true, mode:0o700 });
    allowedRoots=await Promise.all(options.allowedRoots.map(root=>realpath(root)));
    try { budget = JSON.parse(await readFile(join(options.data,'budget.json'),'utf8')); } catch(error:any) { if(error.code!=='ENOENT')throw error; }
    for (const file of await readdir(join(options.data,'jobs'))) {
      if (!/^[a-f0-9-]{36}\.json$/.test(file)) continue;
      const job = JSON.parse(await readFile(join(options.data,'jobs',file),'utf8'));
      if (['queued','running'].includes(job.status)) { job.status='interrupted'; job.message='Service restarted. This preview was not accepted as complete.'; }
      jobs.set(job.id,job);
    }
  })();
  const headers = {
    'Cache-Control':'no-store', 'X-Content-Type-Options':'nosniff', 'Referrer-Policy':'no-referrer',
    'Content-Security-Policy':"default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; media-src 'self' blob: http://100.64.0.12:8010; frame-ancestors 'none'; base-uri 'none'; object-src 'none'"
  };
  const json = (data: unknown, status=200, extra={}) => Response.json(data,{status,headers:{...headers,...extra}});
  // Explicit single-user trusted-tailnet mode. Reachability is the access boundary;
  // browser writes still require the exact app Origin. CLI clients send it too.
  function writeGuard(req: Request) {
    if (req.headers.get('origin') !== options.origin) throw new LabError('origin_denied','This request did not come from Voice Lab.',403);
  }
  async function body(req: Request) {
    const text = await req.text();
    if (text.length > 2048) throw new LabError('too_large','Request is too large.',413);
    try { return JSON.parse(text); } catch { throw new LabError('invalid_json','Send a valid JSON request.'); }
  }
  async function draftList() {
    const drafts=[];
    for (const id of await readdir(join(options.data,'drafts'))) {
      if (!/^[a-f0-9-]{36}$/.test(id)) continue;
      const manifest=JSON.parse(await readFile(join(options.data,'drafts',id,'manifest.json'),'utf8'));
      drafts.push({id,name:manifest.name,created:manifest.created,recipe:manifest.recipe,job:manifest.job,voice_id:manifest.canonical_voice_id});
    }
    return drafts.sort((a,b)=>b.created.localeCompare(a.created));
  }
  async function perform(job: Job) {
    try {
      const result = await options.render(job.recipe,job.id,async()=>{
        const day=new Date().toISOString().slice(0,10);
        if(budget.day!==day) budget={day,used:0};
        if(budget.used>=options.maxDaily) throw new LabError('budget_exhausted','The daily preview budget is used. Ask the operator to change it.',429);
        budget.used++; await atomic(join(options.data,'budget.json'),budget);
        job.status='running'; await atomic(join(options.data,'jobs',job.id+'.json'),job);
      });
      if(result.basis!==options.info.space || result.model!==options.info.model.revision || !Number.isFinite(result.seconds) || result.seconds < 2 || result.seconds > 18.5 || result.wave.length>2400000 || !validWave(result.wave,result.seconds)) throw new LabError('invalid_output','The provider returned an incompatible or incomplete preview.',502);
      job.seconds=result.seconds;job.sha256=digest(result.wave);
      let frozen;try{frozen=captureConditioning(result,job.recipe,options.info,job.sha256);}catch{throw new LabError('invalid_conditioning','The provider returned incompatible frozen conditioning. No voice was saved.',502);}
      if(options.info.capabilities?.frozen_conditioning&&!frozen)throw new LabError('missing_conditioning','The provider did not capture the vector used for this preview.',502);
      if(frozen){await writeFile(join(options.data,'jobs',job.id+'.'+frozen.kind+'.json'),frozen.raw,{flag:'wx',mode:0o600});job.frozen={kind:frozen.kind,sha256:digest(frozen.raw)};}
      await writeFile(join(options.data,'jobs',job.id+'.wav'),result.wave,{flag:'wx',mode:0o600});
      job.status='ready';
    } catch(error) {
      job.status='failed';job.error=error instanceof LabError?error.code:'preview_failed';
      job.message=error instanceof LabError?error.message:'Preview failed. No fallback or replacement audio was used.';
    } finally { await atomic(join(options.data,'jobs',job.id+'.json'),job); active=null; }
  }
  function validateSave(value:any) {
    if(typeof value?.name!=='string' || value.name.trim().length<1 || value.name.length>60 || /[\u0000-\u001f\u007f]/.test(value.name) || typeof value.job!=='string' || !/^[a-f0-9-]{36}$/.test(value.job) || Object.keys(value).sort().join(',')!=='job,name') throw new LabError('invalid_name','Give the draft a name of 1–60 characters.');
  }
  async function saveDraft(value: any) {
    validateSave(value);
    const job=jobs.get(value.job);
    if(!job || job.status!=='ready') throw new LabError('no_preview','Save a completed preview, not the current unrendered sliders.');
    const name=value.name.trim().normalize('NFC');
    if((await draftList()).some(d=>d.name.toLocaleLowerCase()===name.toLocaleLowerCase())) throw new LabError('name_conflict','That draft name exists. Choose a different name.',409);
    const wave=await readFile(join(options.data,'jobs',job.id+'.wav'));
    if(digest(wave)!==job.sha256) throw new LabError('audio_changed','The preview audio changed; it was not saved.',409);
    if(job.recipe.space!==options.info.space)throw new LabError('incompatible_space','This preview belongs to an earlier space; it was not relabelled or regenerated.',409);
    let frozenRaw:string|undefined,frozenDocument:any;
    if(job.frozen){frozenRaw=await readFile(join(options.data,'jobs',job.id+'.'+job.frozen.kind+'.json'),'utf8');if(digest(frozenRaw)!==job.frozen.sha256)throw new LabError('conditioning_changed','The captured vector changed; no draft was saved.',409);frozenDocument=JSON.parse(frozenRaw);}
    if(options.info.capabilities?.frozen_conditioning&&!frozenRaw)throw new LabError('older_preview','This older preview has no captured vector. Its audio remains intact; no regeneration was performed.',409);
    const id=randomUUID(), temporary=join(options.data,'drafts','pending-'+id), destination=join(options.data,'drafts',id);
    await mkdir(temporary,{mode:0o700});
    await writeFile(join(temporary,'reference.wav'),wave,{flag:'wx',mode:0o600});
    await writeFile(join(temporary,'reference.txt'),job.frozen?.kind==='native'?frozenDocument.text:options.info.text+'\n',{flag:'wx',mode:0o600});
    if(frozenRaw)await writeFile(join(temporary,job.frozen?.kind==='native'?'voice.json':'frozen-conditioning.json'),frozenRaw,{flag:'wx',mode:0o600});
    for(const [name,path] of Object.entries(job.frozen?.kind==='native'?{}:options.recipeFiles||{})) {
      if(!['basis.npz','conditioning.safetensors'].includes(name))throw new LabError('invalid_artifact','Unexpected recipe artifact.',500);
      const bytes=await readFile(path);
      if(name==='basis.npz' && digest(bytes)!==options.info.space)throw new LabError('basis_changed','The PCA basis changed; no draft was promoted.',409);
      await writeFile(join(temporary,name),bytes,{flag:'wx',mode:0o600});
    }
    const manifest={version:1,id,name,created:new Date().toISOString(),job:job.id,recipe:job.recipe,model:options.info.model,basis_sha256:options.info.space,reference_sha256:job.sha256,language:'German',canonical_voice_id:null,registry_promoted:false,neural_training:false,frozen:job.frozen||null};
    await atomic(join(temporary,'manifest.json'),manifest);
    await rename(temporary,destination);
    return manifest;
  }
  async function saveReusable(value:any) {
    validateSave(value);
    if(!options.promote||!options.info.capabilities?.registry_promote)throw new LabError('promotion_unavailable','Canonical saving is not available for this provider.',409);
    let manifest:any;
    const existing=(await draftList()).find(d=>d.name.toLocaleLowerCase()===String(value?.name).trim().toLocaleLowerCase());
    if(existing&&existing.job===value?.job)manifest=JSON.parse(await readFile(join(options.data,'drafts',existing.id,'manifest.json'),'utf8'));
    else manifest=await saveDraft(value);
    if(manifest.registry_promoted)return manifest;
    if(manifest.frozen?.kind!=='native')throw new LabError('incompatible_conditioning','This draft is not a native frozen bundle; no reference fallback was used.',409);
    const directory=join(options.data,'drafts',manifest.id);
    let id:string;
    try{id=await options.promote(directory,manifest.name,manifest.id);}catch{throw new LabError('promotion_failed','The exact draft is saved, but registry promotion failed. Retry Save; no audio will be regenerated.',503);}
    if(!/^[A-Za-z0-9_-]{1,128}$/.test(id))throw new LabError('invalid_identity','The registry returned an invalid identity.',502);
    manifest.canonical_voice_id=id;manifest.registry_promoted=true;
    await atomic(join(directory,'manifest.json'),manifest);return manifest;
  }
  async function media(req:Request,id:string) {
    let path=options.media.get(id);
    if(!path && id.startsWith('job-')) {
      const job=jobs.get(id.slice(4));if(job?.status==='ready')path=join(options.data,'jobs',job.id+'.wav');
    }
    if(!path && id.startsWith('draft-') && /^[a-f0-9-]{36}$/.test(id.slice(6))) {
      try{path=await realpath(join(options.data,'drafts',id.slice(6),'reference.wav'));}catch(error:any){if(error.code!=='ENOENT')throw error;}
    }
    if(!path && id.startsWith('rating-') && options.ratings)path=await options.ratings.audio(id.slice(7));
    if(!path) return json({error:'not_found'},404);
    const resolved=await realpath(path);
    if(!allowedRoots.some(root=>resolved.startsWith(root+sep))) return json({error:'media_denied'},403);
    const file=Bun.file(resolved),size=file.size,range=req.headers.get('range');
    const contentType=id.startsWith('rating-')?'audio/mp4':'audio/wav';
    if(!range)return new Response(file,{headers:{...headers,'Content-Type':contentType,'Accept-Ranges':'bytes'}});
    const match=/^bytes=(\d+)-(\d*)$/.exec(range);
    if(!match)return new Response(null,{status:416,headers:{...headers,'Content-Range':`bytes */${size}`}});
    const start=Number(match[1]),end=Math.min(match[2]?Number(match[2]):size-1,size-1);
    if(start>end || start>=size)return new Response(null,{status:416,headers:{...headers,'Content-Range':`bytes */${size}`}});
    return new Response(file.slice(start,end+1),{status:206,headers:{...headers,'Content-Type':contentType,'Content-Range':`bytes ${start}-${end}/${size}`,'Content-Length':String(end-start+1),'Accept-Ranges':'bytes'}});
  }
  return async function fetch(req: Request): Promise<Response> {
    try {
      await ready;
      if(req.headers.get('host')!==new URL(options.origin).host) return json({error:'host_denied'},421);
      const url=new URL(req.url),path=url.pathname;
      if(req.method==='GET' && path==='/health')return json({app:'spqx-voice-lab',status:'up',experimental:true});
      if(req.method==='GET' && path==='/api/ratings')return options.ratings?json(await options.ratings.state()):json({error:'ratings_unavailable',message:'No audition bank is configured. Voice shaping is still available.'},503);
      if(req.method==='POST' && /^\/api\/ratings\/c\d{3}$/.test(path)){
        writeGuard(req);const value=await body(req);
        if(!options.ratings)throw new LabError('ratings_unavailable','No audition bank is configured.',503);
        return json(await options.ratings.save(path.split('/').pop()!,value),201);
      }
      if(req.method==='GET' && path==='/api/state')return json({...options.info,ratings_enabled:!!options.ratings,access:'trusted-tailnet',jobs:[...jobs.values()].sort((a,b)=>b.created.localeCompare(a.created)).slice(0,20),drafts:await draftList()});
      if(req.method==='POST' && path==='/api/preview') {
        writeGuard(req);const recipe=validateRecipe(await body(req),options.info.space);
        if(active)throw new LabError('preview_busy','One preview is already running. It will keep its original settings.',409);
        const job:Job={id:randomUUID(),status:'queued',recipe,created:new Date().toISOString()};
        active=job.id;jobs.set(job.id,job);await atomic(join(options.data,'jobs',job.id+'.json'),job);
        void perform(job);return json(job,202);
      }
      if(req.method==='GET' && /^\/api\/jobs\/[a-f0-9-]{36}$/.test(path)) {
        const job=jobs.get(path.split('/').pop()!);return job?json(job):json({error:'not_found'},404);
      }
      if(req.method==='POST' && (path==='/api/drafts'||path==='/api/voices')) {
        writeGuard(req);const value=await body(req);
        if(saving)throw new LabError('draft_busy','Another draft save is finishing; try again.',409);
        saving=true;try{return json(await (path==='/api/voices'?saveReusable(value):saveDraft(value)),201);}finally{saving=false;}
      }
      if(req.method==='GET' && /^\/media\/[a-zA-Z0-9-]+$/.test(path))return await media(req,path.slice(7));
      const staticFiles:Record<string,string>={'/':'index.html','/index.html':'index.html','/app.js':'app.js','/styles.css':'styles.css','/tokens.css':'tokens.css','/fixture.json':'fixture.json','/ratings':'ratings.html','/ratings.html':'ratings.html','/ratings.js':'ratings.js','/ratings.css':'ratings.css','/rating-fixture.json':'rating-fixture.json'};
      if(req.method==='GET' && staticFiles[path])return new Response(Bun.file(join(options.publicDir,staticFiles[path])),{headers});
      return json({error:'not_found'},404);
    } catch(error) {const known=error instanceof LabError||error instanceof RatingError;return json({error:known?error.code:'internal_error',message:known?error.message:'The request failed safely.'},known?error.status:500);}
  };
}
