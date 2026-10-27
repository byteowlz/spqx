import {chromium} from '@playwright/test';
import {join} from 'node:path';
import {mkdir,rm,writeFile,readFile} from 'node:fs/promises';
import {curationFixture} from '../curation-fixture';
import {CurationBank} from '../curation';
import {createApp} from '../core';
const f=await curationFixture(),bank=new CurationBank(f.root),pub=join(f.root,'public');await mkdir(pub);
for(const name of ['compare.html','compare.js','compare.css','curation-palette.css','curation.css','styles.css','tokens.css'])await Bun.write(join(pub,name),await Bun.file(join(import.meta.dir,'../public',name)).bytes());
const data={takes:[{id:'a',label:'Original synthetic',media:'/media/curation-c001',mechanism:'Synthetic reference',text:'Fixture text',instruction:'Literal <b>instruction</b>'},{id:'b',label:'Alternate synthetic',media:'/media/curation-c002',mechanism:'Synthetic comparison',text:'Fixture text',instruction:''}],finding:'Fixture feedback, not a human judgment.'};await writeFile(join(pub,'compare-data.json'),JSON.stringify(data));
let handler:(r:Request)=>Promise<Response>;const server=Bun.serve({hostname:'127.0.0.1',port:0,fetch:r=>handler(r)});handler=createApp({origin:server.url.origin,data:join(f.root,'app'),info:{},publicDir:pub,media:new Map(),allowedRoots:[f.root],maxDaily:1,curation:bank,render:async()=>{throw Error('No inference');}});
const browser=await chromium.launch({headless:true,args:['--disable-gpu'],executablePath:join(process.env.HOME!,'Library/Caches/ms-playwright/chromium-1248/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')});
const checks:string[]=[],errors:string[]=[];const check=(v:unknown,s:string)=>{if(!v)throw Error(s);checks.push(s);};
try{const page=await browser.newPage({viewport:{width:1280,height:950}});page.on('pageerror',e=>errors.push(e.message));await page.goto(server.url.origin+'/compare');await page.locator('#workspace').waitFor({state:'visible'});
 check(await page.locator('#a-select').inputValue()==='a'&&await page.locator('#b-select').inputValue()==='b','Two distinct defaults');
 await page.locator('#a-audio').click({position:{x:22,y:20}});await page.waitForFunction(()=>document.querySelector<HTMLAudioElement>('#a-audio')!.currentTime>0);check(true,'First audio plays');
 await page.locator('#b-audio').click({position:{x:22,y:20}});await page.waitForFunction(()=>document.querySelector<HTMLAudioElement>('#b-audio')!.currentTime>0);check(await page.locator('#a-audio').evaluate((a:HTMLAudioElement)=>a.paused),'Playing B pauses A');
 await page.locator('#a-title').click();await page.locator('.side').first().locator('summary').click();check(await page.locator('#a-prompt b').count()===0&&(await page.locator('#a-prompt').textContent())?.includes('<b>'),'Prompt is literal text');
 await page.locator('#notes').fill('Fixture listening note');await page.locator('#swap').click();check(await page.locator('#a-select').inputValue()==='b'&&await page.locator('#b-select').inputValue()==='a','Swap changes both selections');check(await page.locator('#notes').inputValue()==='Fixture listening note','Notes retained during swap');
 const output='/tmp/witch-compare-browser';await mkdir(output,{recursive:true});for(const [name,w,h] of [['desktop',1280,950],['phone',390,844]] as const){await page.setViewportSize({width:w,height:h});check(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),name+' no overflow');await page.evaluate(()=>window.scrollTo(0,0));await page.screenshot({path:join(output,name+'.png')});}
 check((await bank.state()).reviewed===0,'No fixture reviews written');check(errors.length===0,'No browser errors');await writeFile(join(output,'proof.json'),JSON.stringify({checks,errors,synthetic:true},null,2));console.log(JSON.stringify({checks:checks.length,errors}));
}finally{await browser.close();server.stop(true);await rm(f.root,{recursive:true,force:true});}
