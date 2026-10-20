import { chromium } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
const origin=process.env.VOICE_LAB_URL||'http://100.64.0.9:8804',out=join(import.meta.dir,'../.impeccable/review');await mkdir(out,{recursive:true});
const browser=await chromium.launch({headless:true,args:['--disable-gpu'],executablePath:join(process.env.HOME!,'Library/Caches/ms-playwright/chromium-1248/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')});
const context=await browser.newContext({viewport:{width:1280,height:1000},reducedMotion:'reduce'}),page=await context.newPage();
const assert=(v:unknown,m:string)=>{if(!v)throw new Error(m);};
try{
 await page.goto(origin);await page.waitForFunction(()=>document.getElementById('preview')?.textContent==='Preview voice');
 assert(await page.locator('#unlock-link,#auth,#key').count()===0,'A key gate remains in the UI');
 assert(await page.locator('#preview').isEnabled(),'Preview is not available without a key');
 const state=await page.evaluate(async()=>await(await fetch('/api/state')).json());assert(state.access==='trusted-tailnet','Wrong access mode');
 const statuses=await page.evaluate(async()=>({
  malformed:(await fetch('/api/preview',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({base:'A',axes:[0,0],space:'incorrect'})})).status,
  unlock:(await fetch('/api/unlock',{method:'POST',headers:{'Content-Type':'application/json'},body:'{}'})).status
 }));assert(statuses.malformed===400&&statuses.unlock===404,'Old auth gate or invalid-input regression');
 const job=state.jobs.find((j:any)=>j.status==='ready'&&j.recipe.base==='A'&&JSON.stringify(j.recipe.axes)==='[0.3,-0.2]');assert(job,'Existing synthetic provider proof unavailable');
 await page.evaluate(async job=>{document.querySelector<HTMLInputElement>('input[name=base][value=A]')!.checked=true;for(const [id,n] of [['pc1',job.recipe.axes[0]],['pc2',job.recipe.axes[1]]] as const){const input=document.getElementById(id) as HTMLInputElement;input.value=String(n);input.dispatchEvent(new Event('input'));}await(window as any).setCurrent({id:job.id,recipe:job.recipe,media:'/media/job-'+job.id});},job);
 await page.waitForFunction(()=>(document.getElementById('current-audio') as HTMLAudioElement).readyState>=2);
 const range=await page.evaluate(async id=>{const r=await fetch('/media/job-'+id,{headers:{Range:'bytes=0-99'}});return {status:r.status,length:(await r.arrayBuffer()).byteLength};},job.id);assert(range.status===206&&range.length===100,'Keyless media/range failed');
 const name='Keyless regression proof';if(!state.drafts.some((d:any)=>d.name===name)){await page.locator('#draft-name').fill(name);await page.locator('#save').click();await page.waitForFunction(()=>document.getElementById('notice')?.textContent?.startsWith('Draft saved with the exact'));}
 await page.reload();await page.waitForFunction(()=>document.getElementById('draft-list')?.textContent?.includes('Keyless regression proof'));
 await page.setViewportSize({width:390,height:844});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Phone overflow');
 const report={key_required:false,fresh_context:true,no_storage_or_credentials:true,preview_button_enabled:true,live_invalid_preview_status:statuses.malformed,removed_unlock_status:statuses.unlock,keyless_media_range:range,keyless_save_reload:true,new_gpu_render:false,existing_synthetic_preview_reused:true};
 await writeFile(join(out,'keyless-proof.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));
}finally{await browser.close();}
