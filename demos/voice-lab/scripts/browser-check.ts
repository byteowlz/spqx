import { chromium } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
const origin=process.env.VOICE_LAB_URL||'http://100.64.0.9:8804';
const home=process.env.HOME!;
const out=join(import.meta.dir,'../.impeccable/review');await mkdir(out,{recursive:true});
const keyProcess=Bun.spawn(['ssh','-o','BatchMode=yes','4090','head -c 64 "$HOME/.local/share/spqx-voice-lab/operator-token"'],{stdout:'pipe',stderr:'pipe'});
const key=(await new Response(keyProcess.stdout).text()).trim();if(await keyProcess.exited || key.length!==64)throw new Error('Operator key could not be read privately.');
const browser=await chromium.launch({headless:true,executablePath:join(home,'Library/Caches/ms-playwright/chromium-1248/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')});
const context=await browser.newContext({viewport:{width:1280,height:1000},reducedMotion:'reduce'});
const page=await context.newPage();const errors:string[]=[];page.on('pageerror',error=>errors.push(error.message));
const assert=(value:unknown,message:string)=>{if(!value)throw new Error(message);};
try{
 await page.goto(origin);await page.waitForFunction(()=>document.getElementById('current-audio')?.getAttribute('src'));
 assert(await page.locator('h1').textContent()==='Dial in a voice.','Heading missing');
 await page.evaluate(async token=>{await (window as any).unlock(token);},key);
 await page.waitForFunction(()=>document.getElementById('unlock-link')?.textContent==='Controls unlocked');
 await page.locator('#pc1').evaluate((input:any)=>{input.value='.3';input.dispatchEvent(new Event('input',{bubbles:true}));});
 await page.locator('#pc2').evaluate((input:any)=>{input.value='-.2';input.dispatchEvent(new Event('input',{bubbles:true}));});
 assert(await page.locator('#dirty').textContent()==='Changes not previewed','Stale audio not disclosed');
 const before=await page.locator('#current-audio').getAttribute('src');
 await page.waitForTimeout(200);assert(await page.locator('#current-audio').getAttribute('src')===before,'Moving a slider changed audio automatically');
 const state=await page.evaluate(async()=>{const r=await fetch('/api/state',{headers:{Authorization:'Bearer '+localStorage.getItem('voice-lab-session')}});return r.json();});
 const prior=state.jobs.find((j:any)=>j.status==='ready'&&JSON.stringify(j.recipe.axes)==='[0.3,-0.2]'&&j.recipe.base==='A');
 if(prior){await page.evaluate(async job=>{await (window as any).setCurrent({id:job.id,recipe:job.recipe,media:'/media/job-'+job.id});},prior);}
 else{await page.locator('#preview').click();await page.locator('#pc1').evaluate((input:any)=>{input.value='.35';input.dispatchEvent(new Event('input',{bubbles:true}));});await page.waitForFunction(()=>document.getElementById('notice')?.textContent==='Preview ready. Listen before keeping it.',{},{timeout:210000});assert(await page.locator('#dirty').textContent()==='Changes not previewed','In-flight changes incorrectly attributed to the audio');}
 await page.waitForFunction(()=>(document.getElementById('current-audio') as HTMLAudioElement).readyState>=2);
 assert((await page.locator('#rendered-settings').textContent())?.includes('PC1 +0.30'),'Rendered recipe missing');
 await page.locator('#pin').click();assert((await page.locator('#pinned-settings').textContent())?.includes('PC1 +0.30'),'A/B pin failed');
 await page.locator('#draft-name').fill('Synthetic browser proof');
 const exists=state.drafts.some((d:any)=>d.name==='Synthetic browser proof');
 if(!exists){await page.locator('#save').click();await page.waitForFunction(()=>document.getElementById('notice')?.textContent?.startsWith('Draft saved with the exact'));}
 await page.reload();await page.waitForFunction(()=>document.getElementById('draft-list')?.textContent?.includes('Synthetic browser proof'));
 await page.getByRole('button',{name:'Open draft'}).first().click();
 await page.waitForFunction(()=>document.getElementById('rendered-settings')?.textContent?.includes('saved draft'));
 assert(await page.locator('#pc1').inputValue()==='0.3','Draft did not restore rendered settings');
 await page.screenshot({path:join(out,'desktop.png'),fullPage:true});
 await page.setViewportSize({width:390,height:844});await page.screenshot({path:join(out,'phone.png'),fullPage:true});
 assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Phone overflow');
 await page.setViewportSize({width:320,height:740});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'320px overflow');
 assert(errors.length===0,'Browser runtime errors: '+errors.join('; '));
 const report={actual_live_preview:!prior,prior_matching_preview_reused:!!prior,authenticated_controls:true,no_inference_on_slider_drag:true,stale_recipe_preserved:true,ab_pin:true,exact_draft_save_reload:true,desktop:[1280,1000],phone:[390,844],narrow_no_overflow:320,audio_decoded:true,audible_or_physical_phone_proven:false,errors};
 await writeFile(join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));
}finally{await browser.close();}
