# Releasing Magic Bill

**This is a procedure, not a habit.** It exists because of one finding, and the
finding is the worst-rated one in the entire audit:

> **POS-C9 / ANDROID-G1 ⛔** — *"All the release signing keys live on this one
> laptop. The Windows updater key and the Android keystore. If this machine
> dies, **nobody can ever ship an update to any existing customer** — Android
> will refuse an app signed by a different key."*
>
> Fix: *"today, put both in a password manager and in CI secrets, and write down
> where they are. **This is a 10-minute job with an enormous downside if
> skipped.**"*

Everything below is written so that a person who is not the author, on a
different computer, can cut a release — because "the person who knows how" is
the same single point of failure as "the laptop that has the key".

---

## 1. The keys

| key | what it signs | where it must be |
|---|---|---|
| **Release signing key** (Ed25519) | the update manifest the counter checks | 1Password → *Magic Bill / Release signing key*, **and** the CI secret `MB_RELEASE_KEY` |
| **Windows code-signing certificate** | the `.exe`, so SmartScreen does not warn | 1Password → *Magic Bill / Windows code signing*, **and** CI secret `MB_WINDOWS_CERT` (+ `MB_WINDOWS_CERT_PASSWORD`) |
| **Android keystore** | the APK/AAB | 1Password → *Magic Bill / Android keystore*, **and** CI secrets `MB_ANDROID_KEYSTORE` (base64) + `MB_ANDROID_KEYSTORE_PASSWORD` + `MB_ANDROID_KEY_ALIAS` |

### ⚠ WHAT TO DO IF A KEY IS LOST

Read this now, not later. **The three answers are different, and one of them
has no recovery.**

**Release signing key (Ed25519) — recoverable, with a release.**
Mint a new keypair, add its public key to `mb_license::snapshot`'s trusted list
**alongside** the old one, and ship that build signed with the OLD key. Once
that build is out, sign with the new key. Counters that took the transitional
build accept both; counters that did not are stuck on their current version
until somebody reinstalls them by hand. **This is why the counter accepts a
LIST of keys rather than one** — the rotation path has to exist before it is
needed.

**Windows code-signing certificate — recoverable, with money and a delay.**
Buy another one. Existing installations update normally; the only symptom is
SmartScreen warning on the new installer until its reputation rebuilds.

**Android keystore — NOT RECOVERABLE. This is the ⛔.**
Android refuses an update signed by a different key, full stop. There is no
appeal and no override. Every existing customer must **uninstall and reinstall**
the app, losing their session and any local state. If Play App Signing is
enabled, Google holds the upload key's counterpart and can reset it — **check
whether it is enabled and write the answer here**:

> Play App Signing enabled: **[ ] yes  [ ] no  — checked by ______ on ______**

Until that box is ticked, assume the worst case.

---

## 2. Cutting a release

1. **Decide the version.** `Cargo.toml`, `tauri.conf.json` and `ui/package.json`
   all carry it and they must agree. It is three numbers — see D96, and
   ANDROID-G4 for what a version *name* cost.
2. **Tag it.** `git tag v1.5.0 && git push --tags`. CI builds from the tag,
   never from a branch, so what shipped can always be rebuilt.
3. CI (`.github/workflows/release.yml`) then:
   * builds `cargo build --release -p magic-bill --features custom-protocol`
     — **the feature is not optional**; without it the binary looks for a dev
     server and opens on "localhost refused to connect";
   * runs the whole suite, clippy, and the four front-end guards;
   * bundles the NSIS installer;
   * code-signs the `.exe`;
   * computes its **SHA-256** (ANDROID-G2 — the counter checks this as well as
     the signature, because a truncated download passes one and fails the other);
   * writes `manifest.json`, signs it with the release key, and publishes both.
4. **Check the manifest before it goes out.** `notes` is shown to a shopkeeper
   in a paragraph. Two sentences, in plain words, about what changed for them —
   not a changelog. Long notes buried v1's install button.

### The manifest

```json
{
  "version": { "major": 1, "minor": 5, "patch": 0 },
  "notes": "Reports open faster, and the day close now prints a slip.",
  "url": "https://releases.magicbill.in/1.5.0/Magic-Bill-1.5.0-setup.exe",
  "sha256": "9f8e7d…",
  "rollout": { "percent": 5, "shops": [] }
}
```

---

## 3. Staging a release

`rollout.percent` starts at **5**. The counter decides with a stable hash of its
own machine id (D101), so a shop in the first 5% stays in it — the cohort does
not reshuffle on every check, which is what makes watching it meaningful.

* **day 1** — 5%. Watch for crash reports and support calls.
* **day 3** — 25%, if nothing came in.
* **day 7** — 100%.

`rollout.shops` names shops that get it whatever the percentage says: use it for
the shop that reported the bug this release fixes.

---

## 4. Pulling a release back

**Set `rollout.percent` to 0 and republish the manifest.** That stops it
reaching anybody who has not taken it yet, within one check.

For shops that already took it, the counter has its own way back and it does not
need us: **Settings → Go back to the previous version** runs the installer it
kept (D97). Two failed starts on a new version roll it back automatically
(D98) — so a release that will not start at all recovers without a phone call.

**What rollback does NOT undo is a database migration.** If the bad release
migrated the shop's data, going back leaves a database a newer schema wrote,
and `mb-db`'s checksum engine will refuse to open it — correctly. The counter's
screen says exactly that and points at "restore last night's backup". **A
release that changes the schema is therefore a release that cannot be rolled
back cleanly, and that has to be weighed before tagging it.**

---

## 5. The one-time setup, if it has not been done

This is the ten minutes POS-C9 is about.

1. Generate the release keypair. `cargo run -p mb-license --example keygen`
   prints a keypair; the public half goes in `mb_license::snapshot`'s
   `PRODUCTION_PUBLIC_KEY`, the private half goes in 1Password and CI.
2. Put the Windows certificate and the Android keystore in 1Password.
3. Add all of them as GitHub Actions secrets.
4. Delete `DEVELOPMENT_SEED` from `mb_license::snapshot` and the test that
   points at it — the test
   `the_development_key_is_still_marked_as_one` fails deliberately once
   `PRODUCTION_PUBLIC_KEY` is set, so it will remind whoever forgets.
5. Tick the Play App Signing box in §1.

**Until step 1 is done, every counter trusts a development key that is in this
repository.** That is fine for a product with no customers and unacceptable for
one with any, and it is the single thing to do before the first real shop.
