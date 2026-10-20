import {chromium} from '@playwright/test';
import {join} from 'node:path';
import {writeFile} from 'node:fs/promises';
const origin='http://100.64.0.9:8804',out=join(import.meta.dir,'../.impeccable/review');
const p=Bun.spawn(['ssh','-o','BatchMode=yes','4090','head -c 64 "$HOME/.local/share/spqx-voice-lab/operator-token"'],{stdout:'pipe',stderr:'pipe'});
const key=(await new Response(p.stdout).text()).trim();if(await p.exited || key.length!==64)throw new Error('Private pairing unavailable');
const browser=await chromium.launch({headless:true,args:['--disable-gpu'],executablePath:join(process.env.HOME!,'Library/Caches/ms-playwright/chromium-1248/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')});
const results=[];
try{for(const [name,width,height] of [['desktop',1280,1000],['phone',390,844]] as const){
 const context=await browser.newContext({viewport:{width,height},reducedMotion:'reduce'}),page=await context.newPage();
 await page.goto(origin);await page.waitForFunction(()=>typeof(window as any).unlock==='function');
 await page.evaluate(async token=>{await(window as any).unlock(token);},key);
 await page.getByRole('button',{name:'Open draft'}).first().click();
 await page.waitForFunction(()=>document.getElementById('rendered-settings')?.textContent?.includes('saved draft'));
 await page.waitForFunction(()=>(document.getElementById('current-audio') as HTMLAudioElement).readyState>=2);
 await page.evaluate(async()=>{await document.fonts.ready;scrollTo(0,0);await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));});
 await page.screenshot({path:join(out,name+'.png'),fullPage:true,animations:'disabled',scale:'css'});
 const dimensions=await page.evaluate(()=>({scrollWidth:document.documentElement.scrollWidth,viewport:innerWidth,height:document.documentElement.scrollHeight,headers:document.querySelectorAll('header').length}));
 if(dimensions.scrollWidth>width||dimensions.headers!==1)throw new Error('Invalid layout/capture source');
 if(name==='desktop'){
  await page.locator('#pc1').focus();await page.keyboard.press('ArrowRight');if(await page.locator('#pc1').inputValue()!=='0.35')throw new Error('Keyboard slider failed');
  const second=await context.newPage();await second.goto(origin);await second.waitForFunction(()=>document.getElementById('unlock-link')?.textContent==='Controls unlocked');await second.close();
 }
 results.push({name,width,height,...dimensions,source:'actual live prototype, synthetic draft, no new inference'});await context.close();
}
await writeFile(join(out,'capture-confirmation.json'),JSON.stringify({captures:results,keyboard_slider:true,operator_new_tab:true,software_browser_render:true,independent_finish_review:false},null,2));console.log(JSON.stringify(results));
}finally{await browser.close();}
