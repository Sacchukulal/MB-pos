// Rich CDP driver: click a CSS selector by real mouse, type real keys, press keys.
// Usage:
//   node cdp.mjs clickSel "<css>"
//   node cdp.mjs type "<css>" "text" [Enter|Escape|...]
//   node cdp.mjs keys Enter ArrowDown ...
const PORT=process.env.CDP_PORT||9222;
const res=await fetch(`http://127.0.0.1:${PORT}/json/list`); const page=(await res.json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise((ok,no)=>{ws.onopen=ok;ws.onerror=no});
let id=0; const wait=new Map(); ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&wait.has(m.id)){wait.get(m.id)(m);wait.delete(m.id);}};
const send=(method,params={})=>new Promise(ok=>{const n=++id;wait.set(n,ok);ws.send(JSON.stringify({id:n,method,params}));});
const evalJs=async(expr)=>{const r=await send('Runtime.evaluate',{expression:expr,returnByValue:true});return r.result?.result?.value;};
const KEY={Enter:{key:'Enter',code:'Enter',windowsVirtualKeyCode:13},Escape:{key:'Escape',code:'Escape',windowsVirtualKeyCode:27},ArrowDown:{key:'ArrowDown',code:'ArrowDown',windowsVirtualKeyCode:40},ArrowUp:{key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38},Backspace:{key:'Backspace',code:'Backspace',windowsVirtualKeyCode:8}};
async function clickRect(sel){
  const r=await evalJs(`(()=>{const e=document.querySelector(${JSON.stringify(sel)});if(!e)return null;const b=e.getBoundingClientRect();return {x:b.x+b.width/2,y:b.y+b.height/2}})()`);
  if(!r){console.log('no el '+sel);return false;}
  await send('Input.dispatchMouseEvent',{type:'mousePressed',x:r.x,y:r.y,button:'left',clickCount:1});
  await send('Input.dispatchMouseEvent',{type:'mouseReleased',x:r.x,y:r.y,button:'left',clickCount:1});
  return true;
}
async function typeText(t){for(const ch of t){await send('Input.dispatchKeyEvent',{type:'keyDown',text:ch});await send('Input.dispatchKeyEvent',{type:'keyUp',text:ch});await new Promise(r=>setTimeout(r,45));}}
async function pressKeys(ks){for(const k of ks){const K=KEY[k];if(!K)continue;await send('Input.dispatchKeyEvent',{type:'rawKeyDown',...K});await send('Input.dispatchKeyEvent',{type:'keyUp',...K});await new Promise(r=>setTimeout(r,140));}}
const [,,mode,a,b,...rest]=process.argv;
if(mode==='clickSel'){console.log(await clickRect(a)?'clicked':'fail');}
else if(mode==='type'){await clickRect(a); await new Promise(r=>setTimeout(r,200)); await pressKeys(['Backspace','Backspace','Backspace','Backspace','Backspace','Backspace','Backspace','Backspace']); await typeText(b); await new Promise(r=>setTimeout(r,400)); if(rest.length)await pressKeys(rest); console.log('typed');}
else if(mode==='keys'){await pressKeys([a,b,...rest].filter(Boolean));console.log('keys sent');}
ws.close();
