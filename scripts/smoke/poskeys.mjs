// Send real key events to the counter over CDP. Usage: node poskeys.mjs "text" [Enter|Escape|ArrowDown...]
const PORT=process.env.CDP_PORT||9222;
const res=await fetch(`http://127.0.0.1:${PORT}/json/list`); const page=(await res.json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise((ok,no)=>{ws.onopen=ok;ws.onerror=no});
let id=0; const wait=new Map(); ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&wait.has(m.id)){wait.get(m.id)(m);wait.delete(m.id);}};
const send=(method,params={})=>new Promise(ok=>{const n=++id;wait.set(n,ok);ws.send(JSON.stringify({id:n,method,params}));});
const KEY={Enter:{key:'Enter',code:'Enter',windowsVirtualKeyCode:13},Escape:{key:'Escape',code:'Escape',windowsVirtualKeyCode:27},ArrowDown:{key:'ArrowDown',code:'ArrowDown',windowsVirtualKeyCode:40},ArrowUp:{key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38},Backspace:{key:'Backspace',code:'Backspace',windowsVirtualKeyCode:8},Tab:{key:'Tab',code:'Tab',windowsVirtualKeyCode:9}};
const text=process.argv[2]||''; const keys=process.argv.slice(3);
for(const ch of text){ await send('Input.dispatchKeyEvent',{type:'keyDown',text:ch}); await send('Input.dispatchKeyEvent',{type:'keyUp',text:ch}); await new Promise(r=>setTimeout(r,40)); }
for(const k of keys){ const K=KEY[k]; if(!K){console.log('unknown key',k);continue;} await send('Input.dispatchKeyEvent',{type:'rawKeyDown',...K}); await send('Input.dispatchKeyEvent',{type:'keyUp',...K}); await new Promise(r=>setTimeout(r,120)); }
console.log('sent'); ws.close();
