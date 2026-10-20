const $=id=>document.getElementById(id);
const fixture=new URLSearchParams(location.search).get('fixture')==='1';
let info,rendered,pinned,busy=false,session;
try{session=localStorage.getItem('voice-lab-session');}catch{}
const number=n=>(n>=0?'+':'')+n.toFixed(2);
const settings=r=>`Voice ${r.base} · PC1 ${number(r.axes[0])} · PC2 ${number(r.axes[1])}`;
const recipe=()=>({base:document.querySelector('input[name=base]:checked').value,axes:[Number($('pc1').value),Number($('pc2').value)],space:info.space});
function notice(message,error=false){$('notice').textContent=message;$('notice').hidden=!message;$('notice').classList.toggle('error',error);}
async function api(path,body){const response=await fetch(path,{method:body?'POST':'GET',headers:{...(body?{'Content-Type':'application/json'}:{}),...(session?{Authorization:'Bearer '+session}:{})},body:body?JSON.stringify(body):undefined});const data=await response.json();if(!response.ok)throw new Error(data.message||data.error||'Request failed.');return data;}
function element(tag,text,className){const node=document.createElement(tag);if(text)node.textContent=text;if(className)node.className=className;return node;}
function draw(){
 if(!info)return;
 const r=recipe(),voice=info.voices.find(v=>v.id===r.base);
 $('pc1-value').textContent=number(r.axes[0])+' σ';$('pc2-value').textContent=number(r.axes[1])+' σ';
 $('source-note').textContent={A:'Source audition kept; no accent reported.',C:'Source audition kept after listening.',D:'Source kept, with a robotic passage still to review.'}[voice.id];
 const dirty=rendered?.recipe && JSON.stringify(r)!==JSON.stringify(rendered.recipe);
 $('dirty').textContent=fixture?'Design preview':dirty?'Changes not previewed':'Settings match preview';$('dirty').classList.toggle('changed',!!dirty);
 $('preview').disabled=busy||fixture||!info.capabilities.live_preview;
 $('preview').textContent=fixture?'Design preview — no live rendering':busy?'Rendering…':info.authorized?'Preview voice':'Unlock to preview';
 $('save').disabled=busy||!info.authorized||!rendered?.id||fixture;
 $('points').replaceChildren();
 const point=(x,y)=>[160+x*48,95-y*33];
 for(const v of info.voices){const [x,y]=point(...v.position);const dot=document.createElementNS('http://www.w3.org/2000/svg','circle');dot.setAttribute('cx',x);dot.setAttribute('cy',y);dot.setAttribute('r',3);dot.setAttribute('class','source-point');const label=document.createElementNS('http://www.w3.org/2000/svg','text');label.setAttribute('x',x+7);label.setAttribute('y',y-6);label.textContent=v.id;$('points').append(dot,label);}
 const [x,y]=point(voice.position[0]+r.axes[0],voice.position[1]+r.axes[1]);$('cursor').setAttribute('cx',x);$('cursor').setAttribute('cy',y);
}
async function setCurrent(take){
 let playback=take.media;
 if(/^\/media\/(job|draft)-/.test(take.media)){const response=await fetch(take.media,{headers:{Authorization:'Bearer '+session}});if(!response.ok)throw new Error('Saved audio could not be loaded. Unlock again if the service restarted.');playback=URL.createObjectURL(await response.blob());}
 rendered={...take,playback};$('current-audio').src=playback;$('rendered-settings').textContent=take.label||settings(take.recipe);draw();
}
function setPinned(take){pinned={...take};$('pinned-audio').src=take.playback||take.media;$('pinned-settings').textContent=take.label||settings(take.recipe);}
function drafts(){
 const list=$('draft-list');list.replaceChildren();
 if(!info.authorized){list.append(element('p','Unlock to see saved drafts. Listening remains available.','muted'));return;}
 if(!info.drafts.length){list.append(element('p','No drafts yet. Preview a voice, listen, then give the result a name.','muted'));return;}
 for(const draft of info.drafts){const row=element('div','','draft'),text=element('div');text.append(element('strong',draft.name),element('p',settings(draft.recipe),'small muted'));const open=element('button','Open draft');open.addEventListener('click',()=>{if(draft.recipe.space!==info.space){notice('That draft belongs to a different conditioning space.',true);return;}document.querySelector(`input[name=base][value=${draft.recipe.base}]`).checked=true;$('pc1').value=draft.recipe.axes[0];$('pc2').value=draft.recipe.axes[1];setCurrent({id:null,recipe:draft.recipe,media:'/media/draft-'+draft.id,label:settings(draft.recipe)+' · saved draft'});notice('Loaded the exact saved audio and its settings. Preview again before saving a new draft.');});row.append(text,open);list.append(row);}
}
function probes(){
 const list=$('probe-list');list.replaceChildren();
 const pairs=[['Baseline modes','Ordinary ICL first, vector-only second.','baseline-icl','baseline-xvector','modes'],['PCA axis 1','Minus first, plus second.','pc1-minus','pc1-plus','pc1'],['PCA axis 2','Minus first, plus second.','pc2-minus','pc2-plus','pc2'],['Saved-reference check','Direct edited vector first, reference clone second.','roundtrip-direct','roundtrip-reloaded','save']];
 for(const [title,order,left,right,id] of pairs){const section=element('div','','probe');section.append(element('strong',title),element('p',order,'small muted'));
  if(fixture){const audio=document.createElement('audio');audio.controls=true;audio.preload='none';audio.src=info.pairUrls[id];section.append(audio);}else{for(const clip of [left,right]){const audio=document.createElement('audio');audio.controls=true;audio.preload='none';audio.setAttribute('aria-label',clip);audio.src='/media/'+clip;section.append(audio);}}
  list.append(section);
 }
}
async function refresh(){info=await api('/api/state');$('unlock-link').textContent=info.authorized?'Controls unlocked':'Unlock controls';drafts();draw();}
async function unlock(token){
 const result=await api('/api/unlock',{token});session=result.session;let remembered=true;
 try{localStorage.setItem('voice-lab-session',session);}catch{remembered=false;}
 $('key').value='';$('auth').hidden=true;await refresh();
 notice(remembered?'Operator controls unlocked. Each preview needs its own admitted GPU capacity.':'Controls unlocked for this tab. Browser storage is blocked, so pairing will not survive closing it.');
}
$('unlock-link').addEventListener('click',()=>{$('auth').hidden=!$('auth').hidden;if(!$('auth').hidden)$('key').focus();});
$('unlock-form').addEventListener('submit',async event=>{event.preventDefault();try{await unlock($('key').value);}catch(error){notice(error.message,true);}});
for(const id of ['pc1','pc2'])$(id).addEventListener('input',draw);
for(const input of document.querySelectorAll('input[name=base]'))input.addEventListener('change',draw);
$('reset').addEventListener('click',()=>{$('pc1').value=0;$('pc2').value=0;draw();});
$('pin').addEventListener('click',()=>{if(rendered)setPinned(rendered);});
for(const player of document.querySelectorAll('audio'))player.addEventListener('play',()=>{for(const other of document.querySelectorAll('audio'))if(other!==player)other.pause();});
document.addEventListener('play',event=>{if(event.target instanceof HTMLAudioElement)for(const other of document.querySelectorAll('audio'))if(other!==event.target)other.pause();},true);
$('preview').addEventListener('click',async()=>{
 if(fixture)return;
 if(!info.authorized){$('auth').hidden=false;$('key').focus();return;}
 busy=true;const requested=recipe();draw();notice('Requesting a bounded preview. No other workload will be evicted.');
 try{let job=await api('/api/preview',requested);for(let i=0;i<120&&['queued','running'].includes(job.status);i++){$('preview-status').textContent=job.status==='queued'?'Checking current GPU admission…':'Rendering these settings. You can keep adjusting; they will not change this job.';await new Promise(resolve=>setTimeout(resolve,1500));job=await api('/api/jobs/'+job.id);}if(job.status!=='ready')throw new Error(job.message||'Preview did not complete within the waiting window. Check again before starting another.');await setCurrent({id:job.id,recipe:job.recipe,media:'/media/job-'+job.id});notice('Preview ready. Listen before keeping it.');}
 catch(error){notice(error.message,true);}finally{busy=false;$('preview-status').textContent='Sliders never start a render on their own.';draw();}
});
$('save-form').addEventListener('submit',async event=>{event.preventDefault();if(!rendered?.id)return;const button=$('save');button.disabled=true;try{await api('/api/drafts',{job:rendered.id,name:$('draft-name').value});$('draft-name').value='';await refresh();notice('Draft saved with the exact preview audio, text and recipe. Not registered or enrolled.');}catch(error){notice(error.message,true);}finally{draw();}});
(async()=>{try{
 info=fixture?await(await fetch('fixture.json')).json():await api('/api/state');
 $('readout').textContent=info.text;$('mode').textContent=fixture?'Studio proposal · read-only':'Experimental PCA space';
 $('unlock-link').hidden=fixture;$('unlock-link').textContent=info.authorized?'Controls unlocked':'Unlock controls';
 const initial={id:null,recipe:{base:'A',axes:[0,0],space:info.space},media:fixture?info.pairUrls.modes:'/media/baseline-xvector',label:fixture?'Earlier baseline pair · no live rendering':'Voice A · PC1 +0.00 · PC2 +0.00 · earlier baseline'};
 setCurrent(initial);setPinned(initial);drafts();probes();
 const params=new URLSearchParams(location.hash.slice(1)),key=params.get('key');if(key&&!fixture){history.replaceState(null,'',location.pathname+location.search);await unlock(key);}
 }catch(error){notice('Voice Lab could not load its live state. No cached result was substituted. '+error.message,true);$('preview').disabled=true;}
})();
