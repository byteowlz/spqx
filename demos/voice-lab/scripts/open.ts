// Pair only the explicitly named user-owned tailnet instance. Never print a key.
const origin=process.env.VOICE_LAB_URL,ssh=process.env.VOICE_LAB_SSH;
if(!origin||!ssh)throw new Error('Set VOICE_LAB_URL and VOICE_LAB_SSH for your instance.');
const url=new URL(origin),parts=url.hostname.split('.').map(Number);
if(url.protocol!=='http:'||url.username||url.password||url.hash||url.search||!(url.hostname==='127.0.0.1'||parts.length===4&&parts[0]===100&&parts[1]>=64&&parts[1]<=127))throw new Error('Use an explicit loopback/tailnet app URL.');
if((await(await fetch(new URL('/health',url))).json()).app!=='spqx-voice-lab')throw new Error('Not a Voice Lab instance.');
const p=Bun.spawn(['ssh','-o','BatchMode=yes',ssh,'head -c 64 "$HOME/.local/share/spqx-voice-lab/operator-token"'],{stdout:'pipe',stderr:'pipe'});
const key=(await new Response(p.stdout).text()).trim();if(await p.exited||!/^[a-f0-9]{64}$/.test(key))throw new Error('Private pairing unavailable.');
url.hash='key='+key;
const opened=Bun.spawn([process.platform==='darwin'?'open':'xdg-open',url.toString()],{stdout:'ignore',stderr:'ignore'});if(await opened.exited)throw new Error('Browser could not be opened.');
console.log('Opened Voice Lab. The key is exchanged for a port-scoped browser session and removed from the address.');
