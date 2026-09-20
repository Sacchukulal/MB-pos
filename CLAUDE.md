# MB-pos — the Windows counter (Tauri 2: Rust + React/TypeScript)

Read this file only. The full rulebook is `../docs/RULES.md`; open it only for the section you
touch. Open problems: `../docs/OPEN_ISSUES.md`. Releasing: `docs/RELEASE.md`. LAN: `docs/LAN_PROTOCOL.md`.

## Layout

- `crates/` — `mb-core` (money, tax, time), `mb-db` (SQLite), `mb-auth`, `mb-lan` (phones on WiFi),
  `mb-license`, `mb-print`, `mb-winprint`. `src-tauri/src/` — one file per feature
  (`billing.rs`, `floor.rs`, `kitchen.rs`, …) and its tests beside it (`billing_tests.rs`).
- `ui/src/` — one folder per screen (`billing/`, `floor/`, `kitchen/`, …); `kit/` is the only
  component set a screen may use; `ipc/` is the only place that talks to Rust.
- `scripts/drive.mjs` drives the real window (`node scripts/drive.mjs text`); use it to check a UI change.
- `src-tauri/src/look_demo.rs` is demo data, not a feature. Never call it from a real path.

## Commands (run from this folder; `cd ui` for npm)

- Dev: `cd ui && npm run dev` in one terminal, `cargo tauri dev` in another (or `./ui/node_modules/.bin/tauri dev`).
- Rust tests, one crate: `cargo test -p mb-db --locked` (also `-p magic-bill` for src-tauri).
- UI check: `cd ui && npm run check` (typecheck + lint + vitest).
- Full gate, same as CI: `node scripts/release.mjs --check`.
- Release: `node scripts/release.mjs`. It picks the next number itself. **Never edit a version number by hand.**

## How to work

1. **Root cause first.** Before any fix, say in one line what is wrong and where it starts.
   A fix that adds an `if` around a symptom is refused. Fix where the wrong value is born.
2. **Small task, small test.** A change in one crate runs that crate's tests. A UI-only change
   runs `npm run check`. The full workspace runs only in `release.mjs --check` before a tag.
3. **One change, one commit.** Commit subject: `type: what changed`, under 60 characters.
   Types: `fix`, `feat`, `ui`, `db`, `print`, `lan`, `ci`, `docs`, `chore`, `release`.
   Body optional, max 2 lines. No stories, no poetry.
4. **Version rule: one step, always.** Fix or feature, the last digit goes up by one:
   1.7.2 -> 1.7.3. A digit never reaches 10; it rolls over: 1.7.9 -> 1.8.0, 1.9.9 -> 2.0.0.
   Never jump, never go backwards, at most one release per day. Bundle the day's fixes.
5. **Ask, do not assume**, for anything in `RULES.md` section 11 (PENDING) or any money/tax rule.
6. **Answer short.** Say what was done and what was left. Do not explain the code back.

## Never

- Never commit screenshots, PNGs, or per-version notes. Notes go in the tag message and `docs/RELEASE.md`.
- Never `unwrap`, `expect`, `panic`, float money, or integer division in Rust (clippy denies them).
- Never compute money in the UI; the UI shows what Rust sends (`npm run lint:money` enforces it).
- Never touch `Cargo.lock` by hand. `--locked` is used everywhere; `release.mjs` refreshes it.
- Never push a tag without `release.mjs`. A red tag is fixed and re-tagged with the same number.
