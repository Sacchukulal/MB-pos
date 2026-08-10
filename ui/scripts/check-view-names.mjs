#!/usr/bin/env node
/**
 * **Two Rust types may not export to the same TypeScript file.**
 *
 * Found at P21 by nearly shipping it. The licensing screen's view model was
 * called `AccountView`, and so is P15's customer account — *"what does this
 * customer owe?"*. Both carry `#[ts(export, export_to =
 * "../../ui/src/ipc/generated/")]`, so both write
 * `generated/AccountView.ts`, and **the last one to run wins.**
 *
 * The failure mode is what makes it worth a script:
 *
 *  * it is silent. `ts-rs` does not warn, `cargo test` passes, `cargo clippy`
 *    passes.
 *  * which type survives depends on **test execution order**, so it can differ
 *    between two runs on the same machine.
 *  * the symptom lands on a screen nobody touched. P15's credit statement would
 *    start failing `tsc` with "property 'customer' does not exist", weeks
 *    later, in a session about something else entirely.
 *
 * D40: *"the rules that erode are enforced by scripts, not by agreement."* This
 * one cannot even be agreed to, because nobody knows the other file exists.
 *
 * NO DEPENDENCIES (see check-tokens.mjs for why).
 *
 *   node scripts/check-view-names.mjs            check ../src-tauri/src
 *   node scripts/check-view-names.mjs <dir>      check somewhere else
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const ROOT = process.argv[2] ?? join('..', 'src-tauri', 'src');

/** Every `.rs` file under `dir`. */
function rustFiles(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) out.push(...rustFiles(path));
    else if (entry.endsWith('.rs')) out.push(path);
  }
  return out;
}

/**
 * Types that carry `#[ts(export...)]`, and where.
 *
 * The attribute and the type declaration are on different lines, and there may
 * be `#[serde(...)]` and doc comments between them — so this walks forward from
 * the attribute to the first `pub struct`/`pub enum`/`pub type`, which is
 * exactly what `ts-rs` itself does.
 */
function exportedTypes(files) {
  const found = new Map();
  for (const file of files) {
    const lines = readFileSync(file, 'utf8').split('\n');
    for (let i = 0; i < lines.length; i += 1) {
      if (!/#\[ts\(.*\bexport\b/.test(lines[i])) continue;
      for (let j = i + 1; j < Math.min(i + 12, lines.length); j += 1) {
        const declaration = /^\s*pub (?:struct|enum|type) ([A-Za-z0-9_]+)/.exec(lines[j]);
        if (!declaration) continue;
        const name = declaration[1];
        if (!found.has(name)) found.set(name, []);
        found.get(name).push(`${relative('.', file)}:${j + 1}`);
        break;
      }
    }
  }
  return found;
}

const files = rustFiles(ROOT);
if (files.length === 0) {
  console.error(`check-view-names: no Rust files under ${ROOT}, so this checked nothing.`);
  process.exit(1);
}

const types = exportedTypes(files);
const clashes = [...types].filter(([, where]) => where.length > 1);

// The scan itself is load-bearing, so it gets an assertion rather than trust —
// the same reason `guard.rs` checks that its own command scan found something.
if (types.size < 20) {
  console.error(
    `check-view-names: the scan found only ${types.size} exported types, so it is ` +
      'broken rather than the code being clean.',
  );
  process.exit(1);
}

if (clashes.length > 0) {
  console.error('Two Rust types export to the same TypeScript file:\n');
  for (const [name, where] of clashes) {
    console.error(`  ${name}.ts  <-  ${where.join('  and  ')}`);
  }
  console.error(
    '\nRename one of them. Whichever test runs last wins, silently, and the ' +
      'screen that breaks is the one nobody touched.',
  );
  process.exit(1);
}

console.log(`check-view-names: ${types.size} exported types, no clashes.`);
