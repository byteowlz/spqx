const $=id=>document.getElementById(id),fixture=new URL(location.href).searchParams.get('fixture')==='1';
let state,index=0,busy=false,pending=null;
const drafts=new Map();
function notice(message,error=false){$('notice').hidden=!message;$('notice').textContent=message;$('notice').classList.toggle('error',error);}
async function api(path,body){const response=await fetch(path,body?{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)}:{});const value=await response.json();if(!response.ok)throw new Error(value.message||'The request failed. Reload saved ratings before trying again.');return value;}
function current(){return state?.candidates[index];}
function operationId(){const b=crypto.getRandomValues(new Uint8Array(16));b[6]=(b[6]&15)|64;b[8]=(b[8]&63)|128;const h=[...b].map(n=>n.toString(16).padStart(2,'0')).join('');return [h.slice(0,8),h.slice(8,12),h.slice(12,16),h.slice(16,20),h.slice(20)].join('-');}
function slider(trait){
 const box=document.createElement('div');box.className='axis';
 const line=document.createElement('div');line.className='inline';
 const label=document.createElement('label');label.htmlFor=trait.id;label.textContent=trait.label;
 const output=document.createElement('output');output.id=trait.id+'-value';output.htmlFor=trait.id;output.value='50';
 const input=document.createElement('input');Object.assign(input,{id:trait.id,name:trait.id,type:'range',min:'0',max:'100',step:'1',value:'50'});
 const scale=document.createElement('div');scale.className='scale';
 for(const text of [trait.low,trait.high]){const span=document.createElement('span');span.textContent=text;scale.append(span);}
 line.append(label,output);box.append(line,input,scale);return box;
}
function chosen(name){const input=document.querySelector(`input[name="${name}"]:checked`);return input?input.value:null;}
function values(){return {gender:chosen('gender'),age_band:chosen('age_band'),scores:Object.fromEntries(state.traits.map(t=>[t.id,Number($(t.id).value)])),naturalness:Number($('naturalness').value),excluded:$('excluded').checked,pronunciation_issue:$('pronunciation-issue').checked,notes:$('rating-notes').value};}
function categoryField(category){
 const box=document.createElement('fieldset');box.className='category';
 const legend=document.createElement('legend');legend.textContent=category.label;box.append(legend);
 const row=document.createElement('div');row.className='choices';
 for(const option of category.options){
  const label=document.createElement('label');label.className='check';
  const input=document.createElement('input');Object.assign(input,{type:'radio',name:category.id,value:option.id});
  const text=document.createElement('span');text.textContent=option.label;label.append(input,text);row.append(label);
 }
 box.append(row);return box;
}
function updateOutputs(){for(const id of [...state.traits.map(t=>t.id),'naturalness'])$(id+'-value').value=$(id).value;}
function draftChanged(){if(!current()||busy)return;updateOutputs();drafts.set(current().id,values());$('draft-state').textContent='Unsaved changes';}
function setBusy(value){busy=value;$('rating-fields').disabled=value;for(const id of ['voice-select','previous','next','unrated','refresh','save-rating','save-only'])$(id).disabled=value||(fixture&&id.startsWith('save'));if(!value){$('previous').disabled=index===0;$('next').disabled=index===state.candidates.length-1;}}
function progress(){const rated=state.candidates.filter(c=>c.rating).length;$('progress').textContent=`${rated} of ${state.candidates.length} rated`;$('bank-status').textContent=`${state.candidates.length} ready · ${state.generation_status==='complete'?'Bank complete':state.generation_status.replaceAll('_',' ')}`;}
function display(next){
 if(busy||!state.candidates.length)return;
 index=next;pending=null;const c=current(),v=drafts.get(c.id)||c.rating||{gender:null,age_band:null,scores:{},naturalness:50,excluded:false,pronunciation_issue:false,notes:''};
 $('voice-title').textContent=`Voice ${String(index+1).padStart(3,'0')}`;$('voice-select').value=c.id;$('rated-tag').textContent=c.rating?'Rated':'Not rated';
 for(const t of state.traits)$(t.id).value=v.scores[t.id]??50;
 for(const category of state.categories)for(const input of document.querySelectorAll(`input[name="${category.id}"]`))input.checked=(v[category.id]===input.value);
 $('naturalness').value=v.naturalness;$('excluded').checked=v.excluded;$('pronunciation-issue').checked=v.pronunciation_issue;$('rating-notes').value=v.notes;
 updateOutputs();$('draft-state').textContent=drafts.has(c.id)?'Unsaved changes':c.rating?`Saved · revision${c.rating.revision}`:'Not saved';
 const audio=$('rating-audio');audio.pause();audio.removeAttribute('src');if(c.media)audio.src=c.media;audio.load();
 $('audio-info').textContent=`${c.seconds.toFixed(2)}s · audition ${index+1} of ${state.candidates.length}`;$('rating-text').textContent=state.text;
 progress();setBusy(false);
}
function selectOptions(){const select=$('voice-select');select.replaceChildren();state.candidates.forEach((c,i)=>{const option=document.createElement('option');option.value=c.id;option.textContent=`Voice ${String(i+1).padStart(3,'0')}${c.rating?' · rated':''}`;select.append(option);});}
function nextUnrated(){return state.candidates.findIndex((c,i)=>i>index&&!c.rating)===-1?state.candidates.findIndex(c=>!c.rating):state.candidates.findIndex((c,i)=>i>index&&!c.rating);}
async function load(){
 if(busy)return;
 if(drafts.size&&!confirm('Reloading discards your unsaved slider changes. Continue?'))return;
 try{
  const previous=current()?.id;state=await api(fixture?new URL('./rating-fixture.json',location.href).pathname:'/api/ratings');drafts.clear();pending=null;
  $('trait-sliders').replaceChildren(...state.traits.map(slider));$('categories').replaceChildren(...state.categories.map(categoryField));selectOptions();
  const available=state.candidates.length>0;$('rating-workbench').hidden=!available;$('empty').hidden=available;progress();
  if(available){let next=state.candidates.findIndex(c=>c.id===previous);if(next<0)next=state.candidates.findIndex(c=>!c.rating);display(next<0?0:next);}
  notice(fixture?'Synthetic review fixture. Saving is disabled.':'');
 }catch(error){notice(error.message,true);if(!state){$('empty').hidden=false;$('progress').textContent='Audition bank unavailable';$('bank-status').textContent='Reload or return to voice shaping.';}}
}
$('rating-form').addEventListener('input',draftChanged);
$('voice-select').addEventListener('change',()=>display(state.candidates.findIndex(c=>c.id===$('voice-select').value)));
$('previous').addEventListener('click',()=>display(Math.max(0,index-1)));
$('next').addEventListener('click',()=>display(Math.min(state.candidates.length-1,index+1)));
$('unrated').addEventListener('click',()=>{const n=nextUnrated();if(n>=0)display(n);else notice('Every ready voice has a saved rating. You can revisit any audition.');});
$('refresh').addEventListener('click',load);$('empty-refresh').addEventListener('click',load);
$('rating-form').addEventListener('submit',async event=>{
 event.preventDefault();if(busy||fixture||!current())return;
 const c=current(),payload={bank:state.bank,audio_sha256:c.audio_sha256,expected_revision:c.rating?.revision||0,...values()},signature=JSON.stringify(payload);
 const missing=state.categories.filter(category=>!payload[category.id]);
 if(missing.length){notice('Choose a value for '+missing.map(category=>category.label).join(' and ')+'. Use Unclear if you cannot tell.',true);return;}
 if(!pending||pending.signature!==signature)pending={signature,body:{...payload,operation_id:operationId()}};
 const stay=event.submitter?.value==='stay';setBusy(true);
 try{
  const rating=await api('/api/ratings/'+c.id,pending.body);c.rating=rating;drafts.delete(c.id);pending=null;selectOptions();setBusy(false);progress();
  if(stay){display(index);notice('Rating saved for this exact audition.');}
  else {const next=nextUnrated();display(next<0?index:next);notice(next<0?'Every ready voice is rated. Thank you.':'Rating saved. Next voice ready.');}
 }catch(error){notice(error.message+' Your scores are still here. Retry unchanged to reuse the same save intent.',true);setBusy(false);}
});
window.addEventListener('beforeunload',event=>{if(drafts.size){event.preventDefault();event.returnValue='';}});
load();
