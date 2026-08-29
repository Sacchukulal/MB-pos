import { readFileSync } from 'node:fs';
// The uiautomator dump to read: UI_XML=<path> (default: ./ui.xml).
const xml = readFileSync(process.env.UI_XML || 'ui.xml','utf8');
const nodes = [...xml.matchAll(/<node [^>]*>/g)].map(m=>m[0]);
const attr=(n,a)=>{const m=n.match(new RegExp(a+'="([^"]*)"'));return m?m[1]:'';};
const centerOf=(n)=>{const b=attr(n,'bounds').match(/\[(\d+),(\d+)\]\[(\d+),(\d+)\]/);if(!b)return null;return [Math.round((+b[1]+ +b[3])/2),Math.round((+b[2]+ +b[4])/2)];};
const [,,mode,arg,nth]=process.argv;
if(mode==='find'){
  const hits=nodes.filter(n=>(attr(n,'text').includes(arg)||attr(n,'content-desc').includes(arg)));
  const n=hits[(+nth||1)-1]; if(!n){console.log('NOTFOUND');process.exit(2);} const c=centerOf(n); console.log(c?c.join(' '):'NOBOUNDS');
} else if(mode==='clickables'){
  nodes.filter(n=>attr(n,'clickable')==='true').forEach(n=>{const c=centerOf(n);console.log((c?c.join(','):'--'),'| t=',JSON.stringify(attr(n,'text')),' d=',JSON.stringify(attr(n,'content-desc')));});
} else if(mode==='texts'){
  nodes.forEach(n=>{const t=attr(n,'text');if(t)console.log(t);});
}
