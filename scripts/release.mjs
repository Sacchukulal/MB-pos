#!/usr/bin/env node
/**
 * One command releases the counter.
 *
 *   node scripts/release.mjs                    1.7.2 -> 1.7.3  (every release, fix or feature)
 *                                               1.7.9 -> 1.8.0, 1.9.9 -> 2.0.0: never a jump, never two digits
 *   node scripts/release.mjs --check            only run the CI gates, change nothing
 *
 * Flags:  --notes "text"   the tag message a shopkeeper reads (default: "Magic Bill X.Y.Z")
 *         --dry-run        show what would change, write nothing
 *         --no-push        commit and tag, but do not push
 *         --skip-tests     bump, commit, tag, push without the gates (only when CI is green already)
 *         --force          allow a second release on the same day (only when the owner says so)
 *
 * It bumps Cargo.toml, Cargo.lock, src-tauri/tauri.conf.json, ui/package.json and
 * ui/package-lock.json together, runs the same checks CI runs, commits "release: vX.Y.Z",
 * tags vX.Y.Z and pushes main + tag. Nobody edits a version number by hand.
 */
import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const flag = (n) => args.includes(n);
const opt = (n) => { const i = args.indexOf(n); return i >= 0 ? args[i + 1] : undefined; };

const files = {
  cargo: "Cargo.toml",
  lock: "Cargo.lock",
  tauri: "src-tauri/tauri.conf.json",
  pkg: "ui/package.json",
  pkgLock: "ui/package-lock.json",
};
const read = (f) => readFileSync(join(root, f), "utf8");
const sh = (cmd, cwd = root) => { console.log(`\n$ ${cmd}`); execSync(cmd, { cwd, stdio: "inherit" }); };
const out = (cmd) => execSync(cmd, { cwd: root, encoding: "utf8" }).trim();
const die = (m) => { console.error(`\nrelease: ${m}`); process.exit(1); };

const current = read(files.cargo).match(/^\[workspace\.package\][\s\S]*?^version = "(\d+\.\d+\.\d+)"/m)?.[1];
if (!current) die("could not find [workspace.package] version in Cargo.toml");

function gates() {
  sh("cargo clippy --workspace --all-targets --locked -- -D warnings");
  sh("cargo test --workspace --locked");
  sh("npm run check", join(root, "ui"));
}

if (flag("--check")) { gates(); console.log("\nrelease: all gates green."); process.exit(0); }

const stray = args.find((a) => !a.startsWith("--") && a !== opt("--notes"));
if (stray) die(`"${stray}": the next version is never chosen. Every release is one step: 1.7.2 -> 1.7.3, 1.7.9 -> 1.8.0, 1.9.9 -> 2.0.0.`);
// One step, always. A digit never reaches 10; it rolls into the digit before it.
let [ma, mi, pa] = current.split(".").map(Number);
pa += 1;
if (pa > 9) { pa = 0; mi += 1; }
if (mi > 9) { mi = 0; ma += 1; }
const next = `${ma}.${mi}.${pa}`;

const cmp = (a, b) => { const x = a.split(".").map(Number), y = b.split(".").map(Number); for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] - y[i]; return 0; };
if (cmp(next, current) <= 0) die(`${next} is not above the current ${current}. Versions never go backwards.`);
if (out("git tag -l v" + next)) die(`tag v${next} already exists`);

const branch = out("git branch --show-current");
if (branch !== "main") die(`you are on "${branch}"; releases go from main`);
if (!flag("--dry-run") && out("git status --porcelain")) die("the working tree is not clean. Commit or stash first.");

const lastTag = out("git tag --sort=-creatordate").split(/\r?\n/)[0] || "";
if (lastTag) {
  const when = out(`git log -1 --format=%cs ${lastTag}`);
  // local date, the same calendar git stamps the tag with (%cs); UTC would say yesterday until 05:30 IST
  const d = new Date();
  const today = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  if (when === today && !flag("--force")) die(`${lastTag} was already released today (${when}). One release per day. Add --force only if the owner says so.`);
}

console.log(`\nrelease: ${current} -> ${next}${flag("--dry-run") ? "  (dry run)" : ""}`);

const edits = {
  [files.cargo]: (s) => s.replace(/^(\[workspace\.package\][\s\S]*?^version = ")(\d+\.\d+\.\d+)(")/m, `$1${next}$3`),
  [files.tauri]: (s) => s.replace(/("version":\s*")(\d+\.\d+\.\d+)(")/, `$1${next}$3`),
  [files.pkg]: (s) => s.replace(/("version":\s*")(\d+\.\d+\.\d+)(")/, `$1${next}$3`),
  // the root entry and packages[""] both carry the app's version; the third and later are dependencies
  [files.pkgLock]: (s) => { let n = 0; return s.replace(/("version":\s*")(\d+\.\d+\.\d+)(")/g, (m, a, v, c) => (n++ < 2 ? `${a}${next}${c}` : m)); },
};
for (const [f, fn] of Object.entries(edits)) {
  const before = read(f), after = fn(before);
  if (before === after) die(`${f}: nothing changed; the version line was not found`);
  console.log(`  ${f}: ok`);
  if (!flag("--dry-run")) writeFileSync(join(root, f), after);
}
if (flag("--dry-run")) { console.log("\nrelease: dry run, nothing written."); process.exit(0); }

// Cargo.lock: re-resolve only the workspace's own crates, nothing from crates.io moves.
sh("cargo update --workspace --offline");

// every file agrees?
const seen = {
  cargo: read(files.cargo).match(/^\[workspace\.package\][\s\S]*?^version = "([^"]+)"/m)[1],
  lock: read(files.lock).match(/name = "magic-bill"\nversion = "([^"]+)"/)[1],
  tauri: JSON.parse(read(files.tauri)).version,
  pkg: JSON.parse(read(files.pkg)).version,
  pkgLock: JSON.parse(read(files.pkgLock)).version,
};
console.log("\nversions:", seen);
for (const [k, v] of Object.entries(seen)) if (v !== next) die(`${k} says ${v}, expected ${next}`);

if (!flag("--skip-tests")) gates();

const notes = opt("--notes") ?? `Magic Bill ${next}`;
sh(`git add ${Object.values(files).join(" ")}`);
sh(`git commit -q -m "release: v${next}"`);
sh(`git tag -a v${next} -m "${notes.replace(/"/g, '\\"')}"`);
if (flag("--no-push")) { console.log(`\nrelease: v${next} committed and tagged, not pushed.`); process.exit(0); }
sh(`git push origin main v${next}`);
console.log(`\nrelease: v${next} pushed. Watch it with:  gh run watch`);
