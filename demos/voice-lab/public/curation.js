'use strict';
const $=id=>document.getElementById(id);
const fixture=new URLSearchParams(location.search).get('fixture')==='1';
document.documentElement.dataset.scheme=matchMedia('(prefers-color-scheme: dark)').matches?'oqto-dark':'oqto-light';
let bank=null,index=0,baseline=null,busy=false,pending=null,serial=0;
const labels={keep:'Kept',reject:'Rejected',unsure:'Unsure'};
function intentId(){const b=crypto.getRandomValues(new Uint8Array(16));b[6]=(b[6]&15)|64;b[8]=(b[8]&63)|128;const h=Array.from(b,n=>n.toString(16).padStart(2,'0')).join('');return h.slice(0,8)+'-'+h.slice(8,12)+'-'+h.slice(12,16)+'-'+h.slice(16,20)+'-'+h.slice(20);}
const formValue=()=>({decision:document.querySelector('input[name=decision]:checked')?.value??null,notes:$('notes').value});
const savedValue=()=>({decision:baseline?.decision??null,notes:baseline?.notes??''});
const dirty=()=>bank!==null&&JSON.stringify(formValue())!==JSON.stringify(savedValue());
function notice(message,error=false){$('notice').textContent=message;$('notice').dataset.error=String(error);}
function buttons(){
 const locked=busy||pending!==null;
 $('fields').disabled=locked||fixture;
 for(const id of ['voice-select','previous','next','unreviewed'])$(id).disabled=locked;
 if(bank){$('previous').disabled=locked||index===0;$('next').disabled=locked||index===bank.candidates.length-1;}
 $('save-next').disabled=fixture||busy||pending!==null||!dirty();
 $('save').disabled=fixture||busy||(!pending&&!dirty());
 $('save').textContent=busy?'Saving…':pending?'Retry Save':'Save only';
 $('refresh').disabled=busy;
 $('draft-state').textContent=busy?'Saving…':pending?'Save outcome unknown':dirty()?'Unsaved changes':baseline?'Saved':'No decision yet';
}
function chooseForm(row){
 baseline=row.review;
 const value=savedValue();
 for(const radio of document.querySelectorAll('input[name=decision]'))radio.checked=radio.value===value.decision;
 if($('notes').value!==value.notes)$('notes').value=value.notes;
}
function show(reset=true){
 const row=bank.candidates[index];
 if(reset)chooseForm(row);
 $('voice-title').textContent='Voice '+row.id.slice(1);
 $('voice-select').value=row.id;
 $('review-tag').textContent=labels[row.review?.decision]??(row.review?.notes?'Notes saved':'Unreviewed');
 const url=new URL(row.media,location.href).href;
 if($('audio').src!==url){$('audio').pause();$('audio').src=url;}
 $('audio-info').textContent=row.seconds.toFixed(2)+'s · audition '+(index+1)+' of '+bank.candidates.length;
 $('audition-text').textContent=bank.text;
 $('workbench').hidden=false;$('unavailable').hidden=true;buttons();
}
function apply(next,discard=false){
 const keepDraft=bank!==null&&!discard&&(dirty()||pending!==null);
 const previousId=bank?.candidates[index]?.id,previousRevision=baseline?.revision??0;
 bank=next;index=Math.max(0,bank.candidates.findIndex(c=>c.id===previousId));
 const selected=bank.candidates[index],options=bank.candidates.map(c=>{
  const option=document.createElement('option');option.value=c.id;option.textContent='Voice '+c.id.slice(1)+' — '+(labels[c.review?.decision]??(c.review?.notes?'Notes saved':'Unreviewed'));return option;
 });$('voice-select').replaceChildren(...options);
 $('progress').textContent=bank.reviewed+' of '+bank.candidates.length+' reviewed';
 $('counts').textContent=['keep','reject','unsure'].map(d=>bank.candidates.filter(c=>c.review?.decision===d).length+' '+({keep:'kept',reject:'rejected',unsure:'unsure'}[d])).join(' · ');
 show(!keepDraft);
 if(keepDraft&&(selected.review?.revision??0)!==previousRevision)notice('This review changed elsewhere. Your draft is kept; Reload saved reviews before replacing it.',true);
}
async function refresh(discard=false){
 if(discard&&(dirty()||pending)&&!confirm('Reload saved reviews and discard your unsaved local changes?'))return;
 const token=++serial;
 try{
  const response=await fetch(fixture?'curation-fixture.json':'/api/curation',{cache:'no-store'}),value=await response.json();
  if(token!==serial)return;
  if(!response.ok||!value.candidates?.length)throw new Error(value.message||'The audition bank is unavailable.');
  if(discard)pending=null;
  apply(value,discard);
  if(fixture){notice('Read-only synthetic review fixture. Saving is disabled.');$('export').hidden=true;}
  else if(discard)notice('Saved reviews reloaded.');
 }catch(error){if(token!==serial)return;notice(error.message+' Your unsaved changes are kept.',true);if(!bank)$('unavailable').hidden=false;}
}
function navigate(next){
 if(!bank||busy||pending)return;
 if(dirty()&&!confirm('Discard this unsaved draft and switch auditions?')){$('voice-select').value=bank.candidates[index].id;return;}
 index=Math.max(0,Math.min(next,bank.candidates.length-1));show();notice(fixture?'Read-only synthetic review fixture. Saving is disabled.':'');$('voice-title').focus();
}
$('voice-select').addEventListener('change',()=>navigate(bank.candidates.findIndex(c=>c.id===$('voice-select').value)));
$('previous').addEventListener('click',()=>navigate(index-1));$('next').addEventListener('click',()=>navigate(index+1));
$('unreviewed').addEventListener('click',()=>{
 for(let step=1;step<=bank.candidates.length;step++){const next=(index+step)%bank.candidates.length;if(bank.candidates[next].review?.decision==null){navigate(next);return;}}
 notice('Every audition has a saved decision. You can revisit any voice.');
});
$('clear').addEventListener('click',()=>{for(const radio of document.querySelectorAll('input[name=decision]'))radio.checked=false;buttons();});
$('review-form').addEventListener('input',buttons);
$('refresh').addEventListener('click',()=>refresh(true));$('retry').addEventListener('click',()=>refresh(true));
$('audio').addEventListener('error',()=>notice('This audition could not be played. Your review is kept; reload to retry.',true));
$('review-form').addEventListener('submit',async event=>{
 event.preventDefault();if(fixture||busy||!bank||(!dirty()&&!pending))return;
 if(!pending){const row=bank.candidates[index];pending={id:row.id,advance:event.submitter?.value==='next',body:{bank:bank.bank,audio_sha256:row.audio_sha256,expected_revision:baseline?.revision??0,operation_id:intentId(),...formValue()}};}
 const intent=pending;busy=true;serial++;buttons();
 try{
  const response=await fetch('/api/curation/'+intent.id,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(intent.body)});
  const value=await response.json();
  if(!response.ok){if(response.status<500)pending=null;throw new Error(value.message||'Save could not be confirmed.');}
  if(value.operation_id!==intent.body.operation_id||value.candidate!==intent.id)throw new Error('The save receipt did not match this audition.');
  const row=bank.candidates.find(c=>c.id===intent.id);row.review=value;baseline=value;pending=null;
  busy=false;apply(bank,true);notice('Review saved.');
  if(intent.advance&&index<bank.candidates.length-1){index++;show();$('voice-title').focus();}
  else if(intent.advance)notice('Review saved. You reached the last audition.');
 }catch(error){notice(error.message+(pending?' Outcome unknown: Retry Save repeats the same intent, or Reload saved reviews to reconcile.':' Your draft is kept.'),true);}
 finally{busy=false;buttons();}
});
addEventListener('beforeunload',event=>{if(dirty()||pending){event.preventDefault();event.returnValue='';}});
setInterval(()=>{if(bank&&!busy&&!pending&&!fixture)void refresh();},15000);
void refresh();
