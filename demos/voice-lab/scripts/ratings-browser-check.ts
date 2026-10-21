import { chromium } from '@playwright/test';
import { mkdtemp, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { tmpdir, homedir } from 'node:os';
import { join } from 'node:path';
import { createApp } from '../core';
import { RatingBank } from '../ratings';
import { ratingFixture } from '../ratings-fixture';
const origin='http://127.0.0.1:8805',root=await mkdtemp(join(tmpdir(),'spqx-rating-browser-'));
const out=process.env.VOICE_RATING_PROOF||join(root,'proof'),shots=join(import.meta.dir,'../.impeccable/review');await mkdir(out,{recursive:true});await mkdir(shots,{recursive:true});
const audio=join(root,'tone.m4a');
const tone=Bun.spawn(['ffmpeg','-v','error','-n','-f','lavfi','-i','sine=frequency=330:duration=2:sample_rate=24000','-c:a','aac','-b:a','96k',audio],{stdout:'pipe',stderr:'pipe'});
if(await tone.exited!==0)throw new Error(await new Response(tone.stderr).text());
await ratingFixture(root,await readFile(audio));let renders=0;
const handler=createApp({origin,data:join(root,'app'),info:{},publicDir:join(import.meta.dir,'../public'),media:new Map(),allowedRoots:[root],maxDaily:1,ratings:new RatingBank(root),render:async()=>{renders++;throw new Error('No inference permitted in rating test');}});
const server=Bun.serve({hostname:'127.0.0.1',port:8805,maxRequestBodySize:4096,fetch:handler});
let browser;const checks:Record<string,unknown>={synthetic_only:true,root};
function assert(value:unknown,message:string){if(!value)throw new Error(message);}
try{
 browser=await chromium.launch({headless:true,args:['--disable-gpu'],executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE||join(homedir(),'Library/Caches/ms-playwright/chromium-1248/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')});
 const context=await browser.newContext({viewport:{width:1440,height:1000}});context.setDefaultTimeout(10000);
 await context.addInitScript(()=>Object.defineProperty(crypto,'randomUUID',{value:undefined})); // Tailnet HTTP has no randomUUID; getRandomValues remains available.
 const page=await context.newPage(),stale=await context.newPage(),errors:string[]=[];page.on('pageerror',e=>errors.push(e.message));
 await Promise.all([page.goto(origin+'/ratings'),stale.goto(origin+'/ratings')]);await Promise.all([page.waitForSelector('#rating-workbench:not([hidden])'),stale.waitForSelector('#rating-workbench:not([hidden])')]);
 assert((await(await fetch(origin+'/api/ratings')).json()).rated===0,'No automatic labels');
 await page.locator('#voice-title').click();await page.locator('#rating-audio').evaluate(async(a:HTMLAudioElement)=>a.play());await page.waitForFunction(()=>document.querySelector<HTMLAudioElement>('#rating-audio')!.currentTime>.15);checks.browser_decode_clock=true;console.log('PASS: audio decode and playback clock');
 await page.locator('#pitch').focus();await page.locator('#pitch').press('End');await page.locator('#pitch').press('ArrowLeft');assert(await page.locator('#pitch').inputValue()==='99','Keyboard range');
 assert((await(await fetch(origin+'/api/ratings')).json()).rated===0,'Slider movement must stay local');checks.no_save_on_drag=true;
 let lost=true;const intents:any[]=[];
 await page.route('**/api/ratings/c001',async route=>{intents.push(route.request().postDataJSON());if(lost){lost=false;await route.fetch();await route.abort('failed');}else await route.continue();});
 await page.locator('#save-only').click();await page.waitForFunction(()=>document.querySelector('#notice')!.textContent!.includes('Your scores are still here'));
 assert((await(await fetch(origin+'/api/ratings')).json()).candidates[0].rating.revision===1,'Commit despite lost ack');
 await page.locator('#save-only').click();await page.waitForFunction(()=>document.querySelector('#draft-state')!.textContent!.includes('revision1'));
 assert(intents.length===2&&intents[0].operation_id===intents[1].operation_id,'Retry must reuse exact intent');checks.lost_ack_idempotent=true;console.log('PASS: lost acknowledgement retry');
 await stale.locator('#pitch').focus();await stale.locator('#pitch').press('Home');await stale.locator('#save-only').click();await stale.waitForFunction(()=>document.querySelector('#notice')!.textContent!.includes('another tab'));
 assert(await stale.locator('#pitch').inputValue()==='0','Conflict must retain local scores');checks.stale_tab_protected=true;console.log('PASS: stale tab conflict');
 await page.reload();await page.waitForSelector('#rating-workbench:not([hidden])');await page.locator('#voice-select').selectOption('c001');await page.waitForFunction(()=>document.querySelector<HTMLInputElement>('#pitch')!.value==='99');checks.reload_restores_scores=true;console.log('PASS: score reload');
 await page.locator('#next').click();await page.locator('#pitch').focus();await page.locator('#pitch').press('Home');await page.locator('#previous').click();await page.locator('#next').click();assert(await page.locator('#pitch').inputValue()==='0','Unsaved local navigation draft');checks.local_navigation_draft=true;
 const range=await fetch(origin+'/media/rating-c001',{headers:{Range:'bytes=0-99'}});assert(range.status===206&&(await range.arrayBuffer()).byteLength===100,'AAC byte range');checks.range_206=true;
 const write=intents[0];const denied=await fetch(origin+'/api/ratings/c001',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(write)});assert(denied.status===403,'Missing origin must fail');checks.origin_required=true;
 assert(!((await readdir(join(root,'app'))).includes('budget.json'))&&renders===0,'No inference/budget side effects');checks.no_inference_or_budget=true;
 assert((await readdir(join(root,'ratings'))).join(',')==='c001.json','Exactly one saved synthetic label');
 const capture=await context.newPage();await capture.goto(origin+'/ratings?fixture=1');await capture.waitForSelector('#rating-workbench:not([hidden])');
 await capture.screenshot({path:join(shots,'rating-r4-desktop.png'),fullPage:true});
 await capture.setViewportSize({width:390,height:844});await capture.screenshot({path:join(shots,'rating-r4-phone.png'),fullPage:true});
 for(const width of [390,320]){await capture.setViewportSize({width,height:844});assert(await capture.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Phone overflow at'+width);}
 checks.phone_widths=[320,390];assert(errors.length===0,'Unexpected page error:'+errors.join(','));checks.page_errors=errors;
 checks.cookies=(await context.storageState()).cookies.length;await context.close();
 await writeFile(join(out,'ratings-browser.json'),JSON.stringify(checks,null,2)+'\n');console.log(JSON.stringify(checks,null,2));
}finally{await browser?.close();server.stop(true);}
