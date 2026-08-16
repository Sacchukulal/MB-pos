#!/usr/bin/env node
/**
 * **Drive the real counter** — P31, and the reason this round found what it
 * found.
 *
 * # Why this exists
 *
 * Every session up to P30.5 proved its work with `cargo test`. All those tests
 * pass. And the owner installed the build, used it for ten minutes, and found
 * six things wrong — because **nothing had ever run the actual window**: not
 * the vite dev server in a browser, where `invoke` does not exist, but the
 * Tauri app, against a real database, with the real IPC boundary in between.
 *
 * WebView2 is Chromium, so it has Chromium's remote-debugging endpoint. Start
 * the counter with it open:
 *
 * ```sh
 * APPDATA=/some/scratch/folder \
 *   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 \
 *   cargo run -p magic-bill
 * ```
 *
 * **Point `APPDATA` somewhere empty.** That is what makes it a genuinely fresh
 * install — D55 and D156: *a session must look at what a SHOP would see, not a
 * seeded demo shop.* Every bug in `NOT_WIRED.md` was found this way.
 *
 * Then:
 *
 * ```sh
 * node scripts/drive.mjs invoke first_run '{}'
 * node scripts/drive.mjs text                       # what is on the screen
 * node scripts/drive.mjs click "text=Start a new shop"
 * node scripts/drive.mjs file scripts/some-scenario.js
 * ```
 *
 * `invoke` goes through `window.__TAURI_INTERNALS__.invoke`, so it is the same
 * road a button takes: the same serialisation, the same guard, the same
 * database. A command that answers here is a command that works.
 *
 * # What it cannot do
 *
 * It cannot see. A screenshot is a separate thing — on Windows,
 * `Get-Process magic-bill`, `GetWindowRect`, `CopyFromScreen` — and the look is
 * still a person's judgement (UI_AFTER.md). This proves behaviour.
 *
 * It also must not open a native modal: a file dialog blocks the command it was
 * called from until somebody dismisses it.
 *
 * NO DEPENDENCIES. Node's built-in `fetch` and `WebSocket` are the whole of it.
 *
 *   node drive.mjs eval "<js expression>"
 *   node drive.mjs invoke <command> '<json args>'
 *   node drive.mjs text                       # visible text of the page
 *   node drive.mjs html "<css selector>"      # one element's markup
 *   node drive.mjs click "<css selector>"     # or click "text=Some Label"
 *   node drive.mjs file <path.js>             # run a script file, print its result
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
  } else {
    console.log('verbs: eval invoke text html click file');
  }
}

await main();
