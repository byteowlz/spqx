// Existing native spqx CLI/session pipeline, never a Python inference adapter or cached-neighbor lookup.
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { LabError, type Recipe } from './core';

export class NativeRenderer {
 private child:ChildProcessWithoutNullStreams|null=null;
 constructor(private executable:string,private config:string,private profile:string,private data:string,private command:(argv:string[],timeout?:number)=>Promise<string>){}
 stop(){if(this.child?.pid&&this.child.exitCode===null)try{process.kill(-this.child.pid,'SIGTERM');}catch{}}
 private async ownedMemory(group:number):Promise<number>{
  const [compute,processes]=await Promise.all([
   this.command(['nvidia-smi','--query-compute-apps=pid,used_gpu_memory','--format=csv,noheader,nounits'],5000),
   this.command(['ps','-axo','pid=,pgid='],5000)
  ]);
  const groups=new Map(processes.trim().split('\n').filter(Boolean).map(line=>line.trim().split(/\s+/).map(Number)) as [number,number][]);
  let total=0;
  for(const line of compute.trim().split('\n').filter(Boolean)){const [pid,memory]=line.split(',').map(s=>s.trim());if(groups.get(Number(pid))===group){const value=Number(memory);if(!Number.isFinite(value))throw new Error('Owned VRAM unavailable');total+=value;}}
  return total;
 }
 async render(recipe:Recipe,id:string,info:any){
  const directory=join(this.data,'jobs',id+'-native'),started=performance.now();
  const argv=['--config',this.config,'lab','preview',info.text,'--space',join(this.profile,'space.json'),'--base',recipe.base,'--offsets='+recipe.axes.join(','),'--seed',String(info.seed),'--out',directory];
  const status=await readFile('/proc/self/status','utf8'),allowed=/^Cpus_allowed_list:\s*(.+)$/m.exec(status)?.[1];
  if(!allowed)throw new LabError('cpu_capacity_unknown','Native preview could not read its allowed CPU set.',503);
  const cpus:number[]=[];for(const part of allowed.split(',')){const [start,end]=part.trim().split('-').map(Number);cpus.push(start);if(end>start)cpus.push(start+1);if(cpus.length>=2)break;}
  const child=spawn('timeout',['--signal=TERM','--kill-after=8s','180','taskset','-c',cpus.slice(0,2).join(','),this.executable,...argv],{detached:true,stdio:['pipe','pipe','pipe'],env:{...process.env,CUDA_VISIBLE_DEVICES:'0',OMP_NUM_THREADS:'2',OPENBLAS_NUM_THREADS:'2'}});
  this.child=child;child.stdin.end();
  let output='',stderr='',done=false,peak=0,monitorError=false,tooLarge=false;
  const monitor=(async()=>{while(!done){try{if(child.pid){peak=Math.max(peak,await this.ownedMemory(child.pid));if(peak>4096)throw new Error('Owned native VRAM above 4GiB claim');}}catch{monitorError=true;this.stop();}await Bun.sleep(250);}})();
  child.stdout.on('data',bytes=>{output+=bytes;if(output.length>1000000){tooLarge=true;this.stop();}});
  child.stderr.on('data',bytes=>{if(stderr.length<16000)stderr+=bytes;});
  try{
   const code=await new Promise<number|null>((resolve,reject)=>{child.on('error',error=>{done=true;reject(error);});child.on('close',resolve);});
   done=true;await monitor;
   if(code!==0||monitorError||tooLarge){await writeFile(join(this.data,'last-provider-error.log'),stderr,{mode:0o600});throw new LabError('native_preview_failed','Native preview failed, exceeded its bounds or could not verify memory. No SDK/reference fallback was used.',502);}
   const metadata=JSON.parse(output.trim());
   const raw=await readFile(join(directory,'voice.json'),'utf8');
   const voice=JSON.parse(raw),wave=await readFile(join(directory,'reference.wav'));
   // PCM validation is performed independently by the application before a job becomes ready.
   const seconds=waveSeconds(wave);
   return {wave,seconds,model:voice.conditioning.scope.model_sha256,basis:voice.conditioning.recipe.space,native_voice_json:raw,elapsed_seconds:(performance.now()-started)/1000,sampled_owned_mib:peak,cpu_affinity:cpus.slice(0,2),metrics:metadata};
  }finally{done=true;await monitor;this.child=null;}
 }
}
function waveSeconds(bytes:Uint8Array){
 const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
 for(let offset=12;offset+8<=bytes.length;){const name=Buffer.from(bytes.subarray(offset,offset+4)).toString(),length=view.getUint32(offset+4,true);if(name==='data')return length/96000;offset+=8+length+(length%2);}
 throw new Error('Missing native PCM data');
}
