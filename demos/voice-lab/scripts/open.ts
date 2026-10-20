const origin=process.env.VOICE_LAB_URL||'http://100.64.0.9:8804';
const url=new URL(origin),parts=url.hostname.split('.').map(Number);
if(url.protocol!=='http:'||url.username||url.password||url.hash||url.search||!(url.hostname==='127.0.0.1'||parts.length===4&&parts[0]===100&&parts[1]>=64&&parts[1]<=127))throw new Error('Use an explicit loopback/tailnet app URL.');
if((await(await fetch(new URL('/health',url))).json()).app!=='spqx-voice-lab')throw new Error('Not a Voice Lab instance.');
const opened=Bun.spawn([process.platform==='darwin'?'open':'xdg-open',url.toString()],{stdout:'ignore',stderr:'ignore'});if(await opened.exited)throw new Error('Browser could not be opened.');
console.log('Opened Voice Lab. No key or pairing required.');
