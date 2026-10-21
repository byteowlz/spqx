import { spawn } from 'node:child_process';
import { mkdir, readFile, writeFile, open, unlink } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { isIP } from 'node:net';
import { createApp, LabError, type Recipe } from './core';
import { NativeRenderer } from './native-renderer';
import { RatingBank } from './ratings';

const here=import.meta.dir;
const args=parseArgs({args:Bun.argv.slice(2),options:{config:{type:'string'},host:{type:'string'},port:{type:'string'},profile:{type:'string'},python:{type:'string'},data:{type:'string'},provider:{type:'string'},'native-cli':{type:'string'},'native-config':{type:'string'},'native-registry':{type:'string'},'rating-bank':{type:'string'},'enable-preview':{type:'boolean'}}}).values;
const configRoot=join(process.env.XDG_CONFIG_HOME||join(homedir(),'.config'),'spqx-voice-lab');
await mkdir(configRoot,{recursive:true,mode:0o700});
const defaultFile=join(configRoot,'config.toml');
try {await writeFile(defaultFile,'host = "127.0.0.1"\nport = 8804\nprofile = ""\npython = ""\nmax_daily = 12\npreview = false\nprovider = "experimental-sdk"\n',{flag:'wx',mode:0o600});}catch(e:any){if(e.code!=='EEXIST')throw e;}
await writeFile(join(configRoot,'schema.json'),await readFile(join(here,'config.schema.json')),{mode:0o600});
const globalConfig=Bun.TOML.parse(await readFile(defaultFile,'utf8')) as any;
let local:any={};try{local=Bun.TOML.parse(await readFile(join(process.cwd(),'config.toml'),'utf8'));}catch{}
let explicit:any={};if(args.config)explicit=Bun.TOML.parse(await readFile(args.config,'utf8'));
const cfg={...globalConfig,...local,...Object.fromEntries(Object.entries({host:process.env.VOICE_LAB_HOST,port:process.env.VOICE_LAB_PORT,profile:process.env.VOICE_LAB_PROFILE,python:process.env.VOICE_LAB_PYTHON,provider:process.env.VOICE_LAB_PROVIDER,native_cli:process.env.VOICE_LAB_NATIVE_CLI,native_config:process.env.VOICE_LAB_NATIVE_CONFIG,native_registry:process.env.VOICE_LAB_NATIVE_REGISTRY,rating_bank:process.env.VOICE_LAB_RATING_BANK}).filter(([,v])=>v!==undefined)),...explicit,...Object.fromEntries(Object.entries(args).filter(([,v])=>v!==undefined).map(([k,v])=>[k.replaceAll('-','_'),v]))};
if(args['enable-preview'])cfg.preview=true;
const host=String(cfg.host),port=Number(cfg.port),profile=resolve(String(cfg.profile||'.'));
const provider=String(cfg.provider||'experimental-sdk'),native=provider==='native-spqx';
const octets=host.split('.').map(Number),tailnet=isIP(host)===4 && octets[0]===100 && octets[1]>=64 && octets[1]<=127;
if(!['experimental-sdk','native-spqx'].includes(provider)||!cfg.profile || (native?(!cfg.native_cli||!cfg.native_config):!cfg.python) || !Number.isInteger(port) || port<1024 || port>65535 || !(host==='127.0.0.1'||tailnet) || !Number.isInteger(Number(cfg.max_daily)) || Number(cfg.max_daily)<1 || Number(cfg.max_daily)>50)throw new Error('Set a local/tailnet host, profile, selected provider/runtime and daily budget1–50.');
const data=resolve(String(cfg.data||join(process.env.XDG_DATA_HOME||join(homedir(),'.local/share'),'spqx-voice-lab')));
await mkdir(data,{recursive:true,mode:0o700});
const lockPath=join(data,'service.lock');
try {const lock=await open(lockPath,'wx',0o600);await lock.writeFile(String(process.pid));await lock.close();}
catch(error:any){if(error.code!=='EEXIST')throw error;const pid=Number(await readFile(lockPath,'utf8'));let alive=true;try{process.kill(pid,0);}catch{alive=false;}if(alive)throw new Error('Another Voice Lab instance may own this data directory.');await unlink(lockPath);const lock=await open(lockPath,'wx',0o600);await lock.writeFile(String(process.pid));await lock.close();}
async function command(argv:string[],timeout=15000){
 const p=Bun.spawn(argv,{stdout:'pipe',stderr:'pipe'});let hard:ReturnType<typeof setTimeout>|undefined;const timer=setTimeout(()=>{p.kill('SIGTERM');hard=setTimeout(()=>p.kill('SIGKILL'),3000);},timeout);
 try{const [stdout,stderr,code]=await Promise.all([new Response(p.stdout).text(),new Response(p.stderr).text(),p.exited]);if(code!==0)throw new Error('Subprocess failed');return stdout;}finally{clearTimeout(timer);if(hard)clearTimeout(hard);}
}
const uv=process.env.VOICE_LAB_UV||join(homedir(),'.cargo/bin/uv');
const dpty=process.env.VOICE_LAB_DPTY||join(homedir(),'.cargo/bin/dpty');
const info=native?JSON.parse(await readFile(join(profile,'profile.json'),'utf8')):JSON.parse(await command([uv,'run','--no-project','--python',cfg.python,'python',join(here,'scripts/export_profile.py'),profile]));
if(native){const measured=JSON.parse(await command([cfg.native_cli,'lab','space',join(profile,'space.json')]));if(measured.space!==info.space||measured.scope.backend!=='ggml'||measured.scope.model_sha256!==info.model.revision||measured.scope.dimension!==1024||measured.max_offset!==0.75||measured.components!==2)throw new Error('Native PCA profile and frozen space do not agree. No SDK fallback.');}
info.provider=provider;info.capabilities.live_preview=!!cfg.preview;info.capabilities.frozen_conditioning=true;
info.capabilities.registry_promote=native;
const nativeRenderer=native?new NativeRenderer(cfg.native_cli,cfg.native_config,profile,data,command):null;
const media=new Map<string,string>();
for(const clip of info.clips)media.set(clip.id,join(profile,'raw',clip.id+'.wav'));
const sourceRoot=join(profile,'../audition-217edd91');
for(const v of info.voices)media.set('source-'+v.id,join(sourceRoot,'raw',v.id+'-reference.wav'));
let activeChild:ReturnType<typeof spawn>|null=null;
function runWorker(recipe:Recipe):Promise<any>{
 return new Promise((resolve,reject)=>{
  const child=spawn('timeout',['--signal=TERM','--kill-after=8s','180',uv,'run','--no-project','--python',cfg.python,'python',join(here,'scripts/sdk_preview.py'),profile],{detached:true,stdio:['pipe','pipe','pipe'],env:{...process.env,HF_HUB_OFFLINE:'1',TRANSFORMERS_OFFLINE:'1',HF_HUB_DISABLE_TELEMETRY:'1',DO_NOT_TRACK:'1',OMP_NUM_THREADS:'2',MKL_NUM_THREADS:'2',TOKENIZERS_PARALLELISM:'false'}});
  activeChild=child;let output='',stderr='',tooLarge=false;
  child.stdin.end(JSON.stringify(recipe));
  child.stdout.on('data',chunk=>{output+=chunk;if(output.length>4000000){tooLarge=true;process.kill(-child.pid!,'SIGTERM');}});
  child.stderr.on('data',chunk=>{if(stderr.length<16000)stderr+=chunk;});
  child.on('error',reject);
  child.on('close',async code=>{
   activeChild=null;
   if(code!==0 || tooLarge){await writeFile(join(data,'last-provider-error.log'),stderr,{mode:0o600});reject(new LabError('provider_failed','The SDK preview failed or timed out; no fallback audio was used.',502));return;}
   try{const marker='VOICE_LAB_RESULT ';const line=output.slice(output.lastIndexOf(marker)+marker.length).trim();resolve(JSON.parse(line));}catch{reject(new LabError('invalid_provider_output','The SDK returned an invalid result.',502));}
  });
 });
}
async function promote(directory:string,name:string,operationId:string){
 const args=[cfg.native_cli,'lab','save','--draft',directory,'--name='+name,'--operation-id',operationId];if(cfg.native_registry)args.push('--registry',cfg.native_registry);
 const result=JSON.parse(await command(args));if(!result.registry_promoted||result.recognition_enrolled!==false)throw new Error('Invalid registry promotion receipt');return result.voice_id;
}
async function render(recipe:Recipe,id:string,admitted:()=>Promise<void>){
 if(!cfg.preview)throw new LabError('preview_disabled','Live rendering is disabled for this instance.',503);
 const receipt=await command([dpty,'loadout','reserve','--claim-gb',native?'4':'3.25','--duration-secs','240','--purpose','VoiceLab explicit synthetic PCA preview '+id]);
 const lease=/^ACCEPTED (rsv-[A-Za-z0-9-]+)/m.exec(receipt)?.[1];
 if(!lease)throw new LabError('gpu_busy','No current GPU capacity was admitted. Your settings are kept; try again later.',503);
 try{
  await admitted();const result=nativeRenderer?await nativeRenderer.render(recipe,id,info):await runWorker(recipe);
  await writeFile(join(data,'jobs',id+'-receipt.json'),JSON.stringify({lease,provider,metrics:{elapsed_seconds:result.elapsed_seconds,sampled_owned_mib:result.sampled_owned_mib,cpu_affinity:result.cpu_affinity,cuda_allocated_gib:result.cuda_allocated_gib,native:result.metrics}}),{mode:0o600});
  return {...result,wave:native?result.wave:Buffer.from(result.wave,'base64')};
 }finally{const receipt=await command([dpty,'loadout','release',lease]);await writeFile(join(data,'jobs',id+'-release.txt'),receipt,{mode:0o600});if(!/released 1 reservation/.test(receipt))throw new LabError('lease_cleanup_unconfirmed','Preview ended but resource cleanup needs operator inspection.',503);}
}
const origin=`http://${host}:${port}`;
const ratingRoot=cfg.rating_bank?resolve(String(cfg.rating_bank)):undefined;
const ratings=ratingRoot?new RatingBank(ratingRoot):undefined;
if(ratings)await ratings.state(); // Invalid configured banks fail startup; no silent substitute.
const handler=createApp({origin,data,info,publicDir:join(here,'public'),media,ratings,allowedRoots:[profile,sourceRoot,data,...(ratingRoot?[ratingRoot]:[])],maxDaily:Number(cfg.max_daily),render,promote:native?promote:undefined,recipeFiles:{'basis.npz':join(profile,'basis.npz'),'conditioning.safetensors':join(profile,'conditioning.safetensors')}});
const server=Bun.serve({hostname:host,port,maxRequestBodySize:4096,fetch:handler});
console.log(`Voice Lab: ${origin} (experimental; trusted-tailnet access, no key)`);
for(const signal of ['SIGINT','SIGTERM'] as const)process.on(signal,async()=>{server.stop();nativeRenderer?.stop();if(activeChild?.pid){try{process.kill(-activeChild.pid,'SIGTERM');}catch{}}await unlink(lockPath).catch(()=>{});setTimeout(()=>process.exit(0),12000);});
