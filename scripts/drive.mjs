#!/usr/bin/env node
/**
 * Drive the real counter.
 *
 * Usage:
 *   node drive.mjs eval "<js expression>"
 *   node drive.mjs invoke <command> '<json args>'
 *   node drive.mjs text
 *   node drive.mjs html "<css selector>"
 *   node drive.mjs click "<css selector>"
 *   node drive.mjs file <path.js>
 */

const PORT = process.env.CDP_PORT || 9222;

async function target() {
  const res = await fetch(`http://127.0.0.1:${PORT}/json/list`);
  const list = await res.json();
  const page = list.find((t) => t.type === 'page');
  if (!page) throw new Error('no page target');
  return page.webSocketDebuggerUrl;
}

async function connect() {
  const url = await target();
  const ws = new WebSocket(url);
  await new Promise((ok, no) => {
    ws.onopen = ok;
    ws.onerror = (e) => no(new Error('ws: ' + e.message));
  });
  let id = 0;
  const waiting = new Map();
  ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && waiting.has(msg.id)) {
      waiting.get(msg.id)(msg);
      waiting.delete(msg.id);
    }
  };
  const send = (method, params = {}) =>
    new Promise((ok) => {
      const n = ++id;
      waiting.set(n, ok);
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  return { send, close: () => ws.close() };
}

export async function evaluate(expression, { awaitPromise = true } = {}) {
  const cdp = await connect();
  const r = await cdp.send('Runtime.evaluate', {
    expression,
    awaitPromise,
    returnByValue: true,
    userGesture: true,
  });
  cdp.close();
  if (r.result?.exceptionDetails) {
    const e = r.result.exceptionDetails;
    return { error: e.exception?.description || e.text };
  }
  if (r.result?.result?.subtype === 'error') {
    return { error: r.result.result.description };
  }
  return { value: r.result?.result?.value };
}

const INVOKE = (cmd, args) => `
  (async () => {
    try {
      const out = await window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${args});
      return { ok: true, out };
    } catch (e) {
      return { ok: false, err: e && typeof e === 'object' ? e : String(e) };
    }
  })()`;

async function main() {
  const [, , verb, a, b] = process.argv;
  if (verb === 'eval') {
    console.log(JSON.stringify(await evaluate(a), null, 2));
  } else if (verb === 'invoke') {
    const r = await evaluate(INVOKE(a, b || '{}'));
    console.log(JSON.stringify(r.value ?? r, null, 2));
  } else if (verb === 'text') {
    const r = await evaluate(`document.body.innerText`);
    console.log(r.value ?? JSON.stringify(r));
  } else if (verb === 'html') {
    const r = await evaluate(`document.querySelector(${JSON.stringify(a)}).outerHTML`);
    console.log(r.value ?? JSON.stringify(r));
  } else if (verb === 'click') {
    const expr = a.startsWith('text=')
      ? `(() => { const t = ${JSON.stringify(a.slice(5))};
           const el = [...document.querySelectorAll('button,a,[role=button],li,label')]
             .find(e => e.innerText.trim() === t || e.innerText.trim().startsWith(t));
           if (!el) return 'not-found'; el.click(); return 'clicked:' + el.tagName; })()`
      : `(() => { const el = document.querySelector(${JSON.stringify(a)});
           if (!el) return 'not-found'; el.click(); return 'clicked'; })()`;
    console.log(JSON.stringify(await evaluate(expr)));
  } else if (verb === 'file') {
    const fs = await import('node:fs');
    const src = fs.readFileSync(a, 'utf8');
    const r = await evaluate(`(async () => { ${src} })()`);
    console.log(typeof r.value === 'string' ? r.value : JSON.stringify(r.value ?? r, null, 2));
  } else if (verb === 'shot') {
    // It can see now.
    const fs = await import('node:fs');
    const cdp = await connect();
    const r = await cdp.send('Page.captureScreenshot', { format: 'png' });
    cdp.close();
    if (!r.result?.data) throw new Error('no image: ' + JSON.stringify(r));
    fs.writeFileSync(a || 'shot.png', Buffer.from(r.result.data, 'base64'));
    console.log(a || 'shot.png');
  } else {
    console.log('verbs: eval invoke text html click file shot');
  }
}

await main();
