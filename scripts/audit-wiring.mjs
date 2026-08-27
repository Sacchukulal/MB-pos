#!/usr/bin/env node
/**
 * Which commands can the shop actually reach?
 *
 * Usage:
 *   node scripts/audit-wiring.mjs
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

const ROOT = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const IPC = join(ROOT, 'src-tauri', 'src', 'ipc.rs');
const CALL_TS = join(ROOT, 'ui', 'src', 'ipc', 'call.ts');
const SCREENS = join(ROOT, 'ui', 'src');

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === 'node_modules' || entry === 'generated' || entry.startsWith('.')) continue;
      walk(full, out);
    } else if (/\.tsx?$/.test(entry)) {
      out.push(full);
    }
  }
  return out;
}

const ipc = readFileSync(IPC, 'utf8').replace(/\r\n/g, '\n');
const block = ipc.match(/macro_rules! commands \{[\s\S]*?generate_handler!\[([\s\S]*?)\n\s*\]\n/);
if (!block) {
  console.error('  could not find the commands!() macro in ipc.rs');
  process.exit(2);
}
const registered = [...block[1].matchAll(/\$crate::(?:[\w]+::)*(\w+)\s*,/g)].map((m) => m[1]);

const callTs = readFileSync(CALL_TS, 'utf8').replace(/\r\n/g, '\n');
const iface = callTs.match(/export interface Commands \{([\s\S]*?)\n\}\n/);
if (!iface) {
  console.error('  could not find the Commands interface in call.ts');
  process.exit(2);
}
const declared = new Set([...iface[1].matchAll(/^ {2}(\w+):\s*\{/gm)].map((m) => m[1]));

/** Where each name is mentioned, outside `call.ts`. */
const seen = new Map();
for (const file of walk(SCREENS)) {
  if (file === CALL_TS) continue;
  const text = readFileSync(file, 'utf8');
  for (const m of text.matchAll(/['"](\w+)['"]/g)) {
    if (!seen.has(m[1])) seen.set(m[1], relative(ROOT, file).split(sep).join('/'));
  }
}

const unreachable = registered.filter((name) => !seen.has(name));
const undeclared = registered.filter((name) => !declared.has(name));
const orphaned = [...declared].filter((name) => !registered.includes(name));

let bad = 0;

if (undeclared.length > 0) {
  bad += undeclared.length;
  console.log(`\n  ${undeclared.length} registered in Rust and not declared in call.ts:`);
  for (const name of undeclared) console.log(`    ${name}`);
}

if (orphaned.length > 0) {
  bad += orphaned.length;
  console.log(`\n  ${orphaned.length} declared in call.ts and not registered in Rust:`);
  for (const name of orphaned) console.log(`    ${name}`);
}

if (unreachable.length > 0) {
  bad += unreachable.length;
  console.log(`\n  ${unreachable.length} command(s) NO SCREEN CAN REACH.`);
  console.log('  A command with a test and no button is a feature the shop');
  console.log('  cannot use. Give it a button, or delete it.');
  for (const name of unreachable) console.log(`    ${name}`);
}

if (bad === 0) {
  console.log(
    `  wiring: clean — all ${registered.length} commands are reachable from a screen`,
  );
} else {
  process.exitCode = 1;
}
