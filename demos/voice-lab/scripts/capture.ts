import {chromium} from '@playwright/test';
import {join} from 'node:path';
import {mkdir,writeFile} from 'node:fs/promises';
const origin=process.env.VOICE_LAB_URL||'http://100.64.0.9:8804',out=join(import.meta.dir,'../.impeccable/review');await mkdir(out,{recursive:true});
const browser=await chromium.launch({headless:true,args:['--disable-gpu'],executablePath:join(process.env.HOME!,'Library/Caches/ms-playwright/chromium-1248/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')});
const results=[];
try{for(const [name,width,height] of [['desktop-r2',1280,1000],['phone-r2',390,844]] as const){
 const context=await browser.newContext({viewport:{width,height},reducedMotion:'reduce'}),page=await context.newPage();
 // Visual evidence is a synthetic-only fixture, not the owner's live draft list.
 await page.goto(origin+'/?fixture=1');await page.waitForFunction(()=>document.getElementById('mode')?.textContent==='Studio proposal · read-only' && document.getElementById('points')?.children.length===6);
 await page.evaluate(async()=>{await document.fonts.ready;await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));});
 if(await page.locator('#unlock-link,#auth,#key').count())throw new Error('Key UI remains');
 await page.screenshot({path:join(out,name+'.png'),fullPage:true,animations:'disabled',scale:'css'});
 if(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth))throw new Error('Overflow');
 results.push({name,viewport:[width,height],fixture:true,private_runtime_data:false});await context.close();
}await writeFile(join(out,'capture-r2.json'),JSON.stringify(results,null,2));console.log(JSON.stringify(results));}finally{await browser.close();}
