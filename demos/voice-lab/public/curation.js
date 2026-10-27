'use strict';
const $=id=>document.getElementById(id);
const fixture=new URLSearchParams(location.search).get('fixture')==='1';
document.documentElement.dataset.scheme=matchMedia('(prefers-color-scheme: dark)').matches?'oqto-dark':'oqto-light';
let bank=null,index=0,baseline=null,busy=false,loading=false,pending=null,serial=0,variant='base';
let selectedBank=new URLSearchParams(location.search).get('bank')||'default';
const labels={keep:'Kept',reject:'Rejected',unsure:'Unsure'};
function intentId(){const b=crypto.getRandomValues(new Uint8Array(16));b[6]=(b[6]&15)|64;b[8]=(b[8]&63)|128;const h=Array.from(b,n=>n.toString(16).padStart(2,'0')).join('');return h.slice(0,8)+'-'+h.slice(8,12)+'-'+h.slice(12,16)+'-'+h.slice(16,20)+'-'+h.slice(20);}
const formValue=()=>({decision:document.querySelector('input[name=decision]:checked')?.value??null,notes:$('notes').value});
const savedValue=()=>({decision:baseline?.decision??null,notes:baseline?.notes??''});
const dirty=()=>bank!==null&&JSON.stringify(formValue())!==JSON.stringify(savedValue());
function notice(message,error=false){$('notice').textContent=message;$('notice').dataset.error=String(error);}
function draftStatus(){
 if(loading)return 'Loading bank…';
 if(busy)return 'Saving…';
 if(pending)return 'Save outcome unknown';
 if(variant==='source')return dirty()?'Editable draft kept':'Comparison only';
 if(dirty())return 'Unsaved changes';
 return baseline?'Saved':'No decision yet';
}
function buttons(){
 const inFlight=busy||loading,locked=inFlight||pending!==null,readOnly=fixture||variant==='source',unsaved=dirty();
 $('fields').disabled=locked||readOnly;
 for(const id of ['bank-select','voice-select','previous','next','unreviewed','variant-base','variant-source'])$(id).disabled=locked;
 if(bank){$('previous').disabled=locked||index===0;$('next').disabled=locked||index===bank.candidates.length-1;}
 $('save-next').disabled=readOnly||locked||!unsaved;
 $('save').disabled=readOnly||inFlight||(!pending&&!unsaved);
 $('save').textContent=busy?'Saving…':pending?'Retry Save':'Save only';
 $('refresh').disabled=inFlight;$('draft-state').textContent=draftStatus();
}
function chooseForm(row){
 baseline=row.review;const value=savedValue();
 for(const radio of document.querySelectorAll('input[name=decision]'))radio.checked=radio.value===value.decision;
 if($('notes').value!==value.notes)$('notes').value=value.notes;
}
function show(reset=true){
 const row=bank.candidates[index];if(reset)chooseForm(row);
 if(!row.source_media)variant='base';
 const source=variant==='source';
 $('voice-title').textContent='Voice '+row.id.slice(1);$('voice-select').value=row.id;
 $('review-tag').textContent=labels[row.review?.decision]??(row.review?.notes?'Notes saved':'Unreviewed');
 $('source-switch').hidden=!row.source_media;$('variant-note').hidden=!row.source_media;
 $('variant-base').setAttribute('aria-pressed',String(!source));$('variant-source').setAttribute('aria-pressed',String(source));
 $('variant-note').textContent=source?'Original design comparison only. Switch back to Editable voice to review it.':'Reviews apply to the editable voice. The original design is a separate comparison.';
 $('review-title').textContent=row.source_media?'Keep the editable voice?':'Would you keep it?';
 const url=new URL(source?row.source_media:row.media,location.href).href;
 if($('audio').src!==url){$('audio').pause();$('audio').src=url;}
 $('audio').setAttribute('aria-label',source?'Original design comparison':'Current editable voice audition');
 const seconds=source?row.design_seconds:row.seconds;
 $('audio-info').textContent=seconds.toFixed(2)+'s · '+(source?'original design':'editable audition')+' '+(index+1)+' of '+bank.candidates.length;
 $('audition-text').textContent=bank.text;$('workbench').hidden=false;$('unavailable').hidden=true;buttons();
}
function apply(next,discard=false){
 const different=bank!==null&&bank.bank!==next.bank;
 const keepDraft=bank!==null&&!different&&!discard&&(dirty()||pending!==null);
 const previousId=different?null:bank?.candidates[index]?.id,previousRevision=baseline?.revision??0;
 if(different)variant='base';
 bank=next;selectedBank=bank.selected_bank||'default';index=Math.max(0,bank.candidates.findIndex(c=>c.id===previousId));
 const available=bank.banks||[{key:'default',title:bank.title||'Voice exploration',count:bank.candidates.length}];
 $('bank-picker').hidden=available.length<2;
 $('bank-select').replaceChildren(...available.map(b=>{const o=document.createElement('option');o.value=b.key;o.textContent=b.title+' · '+b.count;o.selected=b.key===selectedBank;return o;}));
 $('voice-select').replaceChildren(...bank.candidates.map(c=>{const o=document.createElement('option');o.value=c.id;o.textContent='Voice '+c.id.slice(1)+' — '+(labels[c.review?.decision]??(c.review?.notes?'Notes saved':'Unreviewed'));return o;}));
 $('progress').textContent=bank.reviewed+' of '+bank.candidates.length+' reviewed';
 $('counts').textContent=['keep','reject','unsure'].map(d=>bank.candidates.filter(c=>c.review?.decision===d).length+' '+({keep:'kept',reject:'rejected',unsure:'unsure'}[d])).join(' · ');
 if(!fixture){const url=new URL(location.href);if(selectedBank==='default')url.searchParams.delete('bank');else url.searchParams.set('bank',selectedBank);history.replaceState(null,'',url);}
 show(!keepDraft);
 if(keepDraft&&(bank.candidates[index].review?.revision??0)!==previousRevision)notice('This review changed elsewhere. Your draft is kept; Reload saved reviews before replacing it.',true);
}
async function refresh(discard=false,key=selectedBank,confirmed=false){
 if(discard&&!confirmed&&(dirty()||pending)&&!confirm('Reload saved reviews and discard your unsaved local changes?'))return false;
 const token=++serial;
 try{
  const response=await fetch(fixture?'curation-fixture.json':'/api/curation?bank='+encodeURIComponent(key),{cache:'no-store'}),value=await response.json();
  if(token!==serial)return false;
  if(!response.ok||!value.candidates?.length)throw new Error(value.message||'The audition bank is unavailable.');
  if(discard)pending=null;apply(value,discard);
  if(fixture){notice('Read-only synthetic review fixture. Saving is disabled.');$('export').hidden=true;}
  else {$('export').href='/api/curation?bank='+encodeURIComponent(selectedBank);if(discard)notice('Saved reviews reloaded.');}
  return true;
 }catch(error){if(token!==serial)return false;notice(error.message+' Your unsaved changes are kept.',true);if(!bank)$('unavailable').hidden=false;else $('bank-select').value=selectedBank;return false;}
}
function navigate(next){
 if(!bank||busy||loading||pending)return;
 if(dirty()&&!confirm('Discard this unsaved draft and switch auditions?')){$('voice-select').value=bank.candidates[index].id;return;}
 index=Math.max(0,Math.min(next,bank.candidates.length-1));variant='base';show();notice(fixture?'Read-only synthetic review fixture. Saving is disabled.':'');$('voice-title').focus();
}
$('bank-select').addEventListener('change',async()=>{
 const key=$('bank-select').value;
 if(busy||loading||pending||(dirty()&&!confirm('Discard this unsaved draft and switch banks?'))){$('bank-select').value=selectedBank;return;}
 loading=true;buttons();await refresh(true,key,true);loading=false;buttons();
});
for(const [id,value] of [['variant-base','base'],['variant-source','source']])$(id).addEventListener('click',()=>{if(busy||loading||pending)return;variant=value;show(false);});
$('voice-select').addEventListener('change',()=>navigate(bank.candidates.findIndex(c=>c.id===$('voice-select').value)));
$('previous').addEventListener('click',()=>navigate(index-1));$('next').addEventListener('click',()=>navigate(index+1));
$('unreviewed').addEventListener('click',()=>{for(let step=1;step<=bank.candidates.length;step++){const next=(index+step)%bank.candidates.length;if(bank.candidates[next].review?.decision==null){navigate(next);return;}}notice('Every audition has a saved decision. You can revisit any voice.');});
$('clear').addEventListener('click',()=>{for(const radio of document.querySelectorAll('input[name=decision]'))radio.checked=false;buttons();});
$('review-form').addEventListener('input',buttons);
$('refresh').addEventListener('click',()=>refresh(true));$('retry').addEventListener('click',()=>refresh(true));
$('audio').addEventListener('error',()=>notice('This audition could not be played. Your review is kept; reload to retry.',true));
$('review-form').addEventListener('submit',async event=>{
 event.preventDefault();if(fixture||busy||loading||variant!=='base'||!bank||(!dirty()&&!pending))return;
 if(!pending){const row=bank.candidates[index];pending={id:row.id,key:selectedBank,advance:event.submitter?.value==='next',body:{bank:bank.bank,audio_sha256:row.audio_sha256,expected_revision:baseline?.revision??0,operation_id:intentId(),...formValue()}};}
 const intent=pending;busy=true;serial++;buttons();
 try{
  const response=await fetch('/api/curation/'+intent.id+'?bank='+encodeURIComponent(intent.key),{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(intent.body)}),value=await response.json();
  if(!response.ok){if(response.status<500)pending=null;throw new Error(value.message||'Save could not be confirmed.');}
  if(value.operation_id!==intent.body.operation_id||value.candidate!==intent.id)throw new Error('The save receipt did not match this audition.');
  bank.candidates.find(c=>c.id===intent.id).review=value;baseline=value;pending=null;busy=false;apply(bank,true);notice('Review saved.');
  if(intent.advance&&index<bank.candidates.length-1){index++;variant='base';show();$('voice-title').focus();}else if(intent.advance)notice('Review saved. You reached the last audition.');
 }catch(error){notice(error.message+(pending?' Outcome unknown: Retry Save repeats the same intent, or Reload saved reviews to reconcile.':' Your draft is kept.'),true);}
 finally{busy=false;buttons();}
});
addEventListener('beforeunload',event=>{if(dirty()||pending){event.preventDefault();event.returnValue='';}});
setInterval(()=>{if(bank&&!busy&&!loading&&!pending&&!fixture)void refresh();},15000);
void refresh();
