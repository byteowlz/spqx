const $=id=>document.getElementById(id);
const theme=matchMedia('(prefers-color-scheme: dark)');function setTheme(){document.documentElement.dataset.scheme=theme.matches?'oqto-dark':'oqto-light';}theme.addEventListener('change',setTheme);setTheme();
let takes=[];
function take(id){return takes.find(t=>t.id===$(id+'-select').value);}
function show(id){
 const t=take(id),audio=$(id+'-audio');audio.pause();audio.src=t.media;audio.setAttribute('aria-label',id.toUpperCase()+': '+t.label);
 $(id+'-mechanism').textContent=t.mechanism;$(id+'-text').textContent=t.text;$(id+'-prompt').textContent=t.instruction||'No instruction: this take uses reference conditioning.';
}
function choose(id){$(id+'-select').addEventListener('change',()=>show(id));$(id+'-audio').addEventListener('play',()=>$(id==='a'?'b-audio':'a-audio').pause());}
$('swap').addEventListener('click',()=>{const a=$('a-select').value;$('a-select').value=$('b-select').value;$('b-select').value=a;show('a');show('b');});
$('copy').addEventListener('click',async()=>{const a=take('a'),b=take('b');try{await navigator.clipboard.writeText('A: '+a.label+'\nB: '+b.label+'\n\n'+$('notes').value);$('notice').textContent='Comparison notes copied.';}catch{$('notice').textContent='Could not copy. Select the notes and copy them manually.';}});
async function start(){
 try{
  const response=await fetch('/compare-data.json',{cache:'no-store'});if(!response.ok)throw Error('unavailable');const data=await response.json();
  if(!Array.isArray(data.takes)||data.takes.length<2||data.takes.some(t=>!t.id||!t.label||!t.media||!t.mechanism||!t.text))throw Error('invalid comparison');takes=data.takes;
  for(const id of ['a','b']){$(id+'-select').replaceChildren(...takes.map(t=>{const option=document.createElement('option');option.value=t.id;option.textContent=t.label;return option;}));choose(id);}
  $('a-select').value=takes[0].id;$('b-select').value=takes[1].id;show('a');show('b');$('finding-text').textContent=data.finding||'';$('workspace').hidden=false;$('notice').textContent='';
 }catch{$('notice').textContent='';$('unavailable').hidden=false;}
}
start();
