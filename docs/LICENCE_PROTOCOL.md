# The licence protocol

**What the counter asks the cloud, what the cloud must answer, and what the
counter does when each call fails.**

Written at P21, as the counter side was built. **P34 implements the cloud side
from this document**, the same way Phase 11 implements the phone from
`LAN_PROTOCOL.md` — with no access to the counter's source and no access to the
reasoning that produced it. Everything needed is here: every call, every field,
every unit, the signature format, and the failure behaviour.

If something in here disagrees with `crates/mb-license`, **this document is
wrong and the code is right** — but say so, out loud, because a protocol
document nobody trusts is worse than none.

---

## 0. The three things that are not negotiable

### 0.1 Billing is never gated

Not by this protocol, not by any answer the cloud can send, not by any failure
of any call in it. The counter's `Feature` enum — the complete list of what a
licence can refuse — has four values and none of them is billing. **A cloud
response cannot stop a restaurant trading**, and any future field that tries to
will be ignored by every counter already shipped.

### 0.2 One idea of "may this shop work", in both programs

> **BACKEND-C1:** *"'Suspend' and 'Revoke' in the admin panel do nothing. Both
> set `licenses.status`. Nothing anywhere reads `status` as a gate ... The admin
> screen even says 'the POS locks at its next status check' — it does not."*

§3 is the algorithm. **The cloud runs the same one**, on the same inputs, in the
same order. A resolver that reaches a different answer from the counter is the
finding happening again with newer code.

### 0.3 Nothing may hang

Every call has a deadline **enforced by the caller** (8 s normally, 1 s at
startup). The cloud may be slow; the counter will stop waiting. An endpoint that
takes longer than 8 s to answer is an endpoint the counter never sees the answer
from, so it must be safe to have been called and abandoned — see §6.

---

## 1. Transport and shape

* HTTPS, JSON, `POST` for everything. No call is idempotent-by-GET because every
  one of them either changes a binding or is rate-limited.
* Bodies are UTF-8 JSON. Money does not appear anywhere in this protocol.
* Every request carries an `Ask`:

```json
{
  "key": "MB-XXXX-XXXX-XXXX",
  "machine": { "value": "4c4c4544-0043-4a10-8033-b8c04f4d3132", "how": "machine-guid" },
  "counter_version": "0.1.0"
}
```

  `machine.how` is one of `machine-guid` · `counter-id` · `generated` ·
  `recorded`. **The cloud must not treat `generated` as untrustworthy** — it
  means the counter could not read a hardware id, which is a support fact, not a
  fraud signal. It is sent so support can see it.

* Timestamps are **integer milliseconds since the Unix epoch, UTC**. Days
  (`renews_on`, `trial_ends_on`) are **integer days since 1970-01-01** — not
  strings, not ISO dates. That is `BusinessDay` and it is how every date in this
  product is stored (D5).

---

## 2. The five calls

### 2.1 `POST /v1/licence/activate`

First activation.

```json
{ "ask": { ... }, "proof": "123456" }
```

> **BACKEND-C6:** *"An unactivated licence accepts any device. Both the
> enrolment program and the old bill-sync program say 'if no device is bound
> yet, accept whatever device id you are given'. So for a licence that has been
> sold but not yet activated, whoever types the key first becomes the counter —
> and can push bills into that restaurant's cloud data."*

**`proof` is that finding's fix and it is mandatory.** It is a one-time code the
cloud sent to the licence's registered mobile or email. The cloud **must** refuse
an activation whose proof is absent, wrong or expired, and must do so for an
unbound licence exactly as strictly as for a bound one. There is no counter
build that can call this without a proof — the parameter is not optional in the
trait.

Refuse with `bound_elsewhere` when the licence is bound to a different machine
and the caller did not ask to move it. **Do not silently rebind.**

Answers `SignedSnapshot` (§4).

### 2.2 `POST /v1/licence/refresh`

The routine check. Body is the `Ask` alone.

**The cloud must check the device binding on every one of these.**

> **BACKEND-C4:** *"Moving a licence to a new PC leaves the old PC with full
> cloud access. ⛔ The resolver checks revocation, subscription, ordering switch,
> staff status and phone blocks. It does not check whether this device is still
> the bound device."*

A refresh from a machine that is not the bound one answers `bound_elsewhere`.
The counter enforces the same thing locally with no network at all, but the
cloud is the side that matters, because the cloud is what the old PC would
otherwise still be writing bills into.

Answers `SignedSnapshot`.

### 2.3 `POST /v1/licence/release`

> **BACKEND-C5:** *"'Deactivate' on the counter is a trap. It only clears the
> key locally. The server still holds the device binding, so the owner then
> cannot activate on a different PC and has to contact support. Every owner who
> tries to move machines hits this."*

Body is the `Ask`. Clears `bound_to` on the licence.

> ### ⚠ RELEASE MUST ALSO REVOKE THE COUNTER'S CLOUD CREDENTIAL
>
> **BACKEND-C4, and this is the box it gets.** Clearing the binding column is
> not enough and was exactly v1's bug: *"That old machine can still push bills,
> write live orders and read the whole menu — for as long as the subscription
> lasts."*
>
> A release, **and a transfer**, must revoke the released machine's cloud
> login/mapping row in the same transaction as the unbind. If the two can
> commit separately, there is a window in which the binding says one thing and
> the credential says another, and that window is the finding.

Answers `204`. **Idempotent**: releasing an already-released machine succeeds.
The counter retries a queued release on every refresh, possibly for days, and a
second release must not be an error.

### 2.4 `POST /v1/licence/transfer`

```json
{ "ask": { ... }, "proof": "123456" }
```

Moves the licence onto the calling machine from wherever it was. POS-A4's
self-service path — *"there was no self-service way to move a licence to a new
machine at all"*.

* `proof` is mandatory, same as activation.
* The **old** machine is released and its credential revoked (the box above).
* **The counter enforces a 30-day cooldown of its own**, in `licence.json`. The
  cloud must enforce one too, server-side, and it is the one that counts: the
  counter's copy is a courtesy that gives a fast refusal with the days left in
  it, and a counter's local file is not a control.

Answers `SignedSnapshot`.

### 2.5 `POST /v1/licence/trial`

```json
{ "ask": { ... }, "contact": "+919812345610" }
```

Starts a self-service trial. Sets `status: "trial"` and a `trial_ends_on`, and
binds the machine.

**A trial converts to paid by the cloud changing `status` to `active` and
sending a new snapshot. Nothing on the counter is re-entered and the binding is
not touched.** If P34 finds itself designing a "convert" call, it has taken a
wrong turn — that is requirement 4 and the counter already behaves correctly
without one.

---

## 3. The answer, and the algorithm both sides run

### 3.1 The licence

```json
{
  "key": "MB-XXXX-XXXX-XXXX",
  "shop_name": "Anna's Kitchen",
  "plan": {
    "code": "restaurant-standard",
    "name": "Restaurant Standard",
    "features": ["reports", "cloud-backup", "mobile-ordering"],
    "limits": { "devices": 4, "terminals": 1 }
  },
  "status": "active",
  "renews_on": 20709,
  "grace_days": null,
  "bound_to": { "value": "...", "how": "machine-guid" },
  "trial_ends_on": null,
  "registered_contact": "+91 98••••••10"
}
```

* `status` ∈ `active` · `trial` · `suspended` · `revoked` · `cancelled`.
* `features` is a list of **stable codes**. A code the counter does not know is
  **ignored, not rejected** — a newer cloud must be able to talk to an older
  till. The four codes a counter understands today are `reports`,
  `cloud-backup`, `mobile-ordering`, `multi-terminal`. **There is no code for
  billing or printing and there never will be** (§0.1).
* A plan is data. Adding "Restaurant Plus, four phones" is a row in the admin
  panel and **never a counter release**.
* `registered_contact` **arrives already masked**. The counter shows it on a
  screen anybody with `reports.view` can open, and the owner's mobile number is
  not something a shop's staff need. Sending the full value is a bug in the
  cloud.

### 3.2 The grace period — the algorithm, in order

> **BACKEND-C3:** *"The counter has 10 days hard-coded. The cloud uses the
> per-licence override, then the global setting, then 10. So if you set a
> customer's grace to 30 days in the admin panel, the counter still locks at 10
> while the phones keep working."*

```
grace = licence.grace_days          (per-licence override)
     ?? snapshot.global_grace_days  (the shop-wide setting)
     ?? 10                          (the last resort, in BOTH programs)
```

`global_grace_days` **travels inside the snapshot** rather than being fetched
separately, so the counter can never hold a stale global against a fresh
licence. There is no other way for the counter to learn it, and no `10` anywhere
in `crates/mb-license` except the one named constant — a test proves it.

### 3.3 Deciding, in order

```
1.  status is suspended / revoked / cancelled  → not operating. TODAY.
                                                  Whatever renews_on says.
2.  status is trial and today > trial_ends_on  → trial ended.
3.  status is trial                            → fine.
4.  today <= renews_on                         → fine.
5.  today <= renews_on + grace                 → in grace, everything works.
6.  otherwise                                  → expired.
```

**Step 1 is the finding.** It comes before every date comparison, in both
programs. A suspended licence with a billing date a year away is not entitled,
today.

**Step 5 removes nothing.** A grace period that quietly withdrew features would
be a lock nobody announced.

---

## 4. `SignedSnapshot`

```json
{
  "payload": "{\"licence\":{...},\"global_grace_days\":15,\"issued_at\":1754...,\"not_after\":1755...,\"max_offline_days\":14}",
  "signature": "base64 of 64 bytes"
}
```

* `payload` is the **exact JSON text that was signed**, transmitted as a string.
  The counter verifies over those bytes and only then parses them. It never
  re-serialises to check — that would make verification depend on field order
  and on which fields the counter's build knows about.
* `signature` is **Ed25519** over `payload`'s UTF-8 bytes, base64 (standard
  alphabet, with padding).
* The counter holds the public key. P34 mints the production keypair and
  replaces the development one; the counter accepts a list of keys so a rotation
  does not need every till updated on the same day.

### 4.1 The two expiries, and why there are two

* `not_after` — wall clock. Whatever the cloud said, the counter must come back.
* `max_offline_days` — measured **from the last successful check** and judged
  **against a high-water mark that only ever moves forward.**

Both must hold. The wall clock is a number the person being gated owns; the
high-water mark is not. Setting the PC's clock back to March defeats the first
and does nothing at all to the second.

Suggested values: `not_after` = 7 days, `max_offline_days` = 14. A shop with a
broken router keeps everything for a fortnight, which is longer than any
router outage and shorter than a billing cycle.

---

## 5. Errors

| code | HTTP | counter's behaviour |
|---|---|---|
| `unreachable` (no response at all) | — | keeps the cached snapshot, keeps billing, shows nothing |
| `not_recognised` | 401 | "that licence key and code did not match" |
| `bound_elsewhere` | 409 | names the other machine's short id |
| `refused` | 403 | **shows the server's own sentence, as-is** |
| `too_soon` | 429 | the cooldown, with days left |
| anything unparseable | any | treated as `unreachable` |

`refused` carries `{"message": "..."}` and **the counter prints that string
without touching it.** The reason a licence was refused is a thing only the
cloud knows, so the cloud writes the sentence. Write it for a shopkeeper, in one
sentence, with what to do next — the same rule D84 puts on refusals a waiter
reads.

**No error stops billing.** Every row above leaves the counter taking orders.

---

## 6. Idempotency and abandoned calls

The counter abandons a call at its deadline and the request may still complete
server-side. So:

* `release` and `transfer` must be safe to have half-happened from the counter's
  point of view — the counter will call them again.
* `activate` **must not** consume the proof code on a request it then fails to
  answer within 8 s, or an owner with a slow connection burns their OTP without
  ever seeing a result.
* A retried `release` for a machine already released answers `204`.

---

## 7. Rate limiting

> **POS-C10:** *"No rate limit or lockout on licence-key activation attempts.
> Keys follow a guessable pattern and a script could try many."*

Per key **and** per IP, on `activate` and `transfer`. The counter's own bucket
covers only the emergency code (five tries, then fifteen minutes); everything
that reaches the network is the cloud's to limit, because a counter's local
counter is not a control against somebody who is not using the counter.

`mb-lan`'s pairing endpoint is the reference: the tightest bucket in the
product, always with a `Retry-After`.

---

## 8. The offline emergency code

Not a call. Support mints a code by hand and reads it out; the counter verifies
it with **no network**.

* 20 characters, Crockford base32, four groups of five: `K7M2Q-9XR4T-BW8HN-3PZ6D`.
* HMAC-SHA256 over `machine_id_bytes || payload_be_u32`, truncated to the top 76
  bits; the payload is the low 24 bits of the code (16 bits issue-day, 8 bits
  hours).
* Valid from the **start of its issue day, UTC**, for `hours` — so a 72-hour
  code gives between two and three days of real life, and support and the shop
  never have to agree about a timezone over the phone.
* Single use per machine, rate limited, and audited on the counter with the
  person who typed it.
* **The secret is compiled into the counter.** Anybody who extracts it can mint
  codes for their own machine. That is the stated, accepted cost of a shop being
  able to bill when its PC dies on a Saturday.

P34 owns the tool support uses. It must never mint a code without recording who
asked and for which machine.

---

## 9. Two things this protocol deliberately does NOT have

### 9.1 No owner-phone binding

> **BACKEND-C7:** *"The owner's phone binding is decorative. The `mobile-device`
> program records which phone the owner uses, the admin panel shows it and
> offers 'Unbind phone' — but nothing ever checks it. An owner can log in on ten
> phones. The admin action does nothing real."*
> **Fix:** *"either enforce it or remove it. A feature that pretends to be a
> control is worse than no feature."*

**Removed.** There is no owner-phone binding in this counter and there must not
be one in the cloud until something enforces it. If P34 finds a column for it in
the old schema, it does not carry over. The enforcing side, if it is ever
wanted, is the Android app's and Phase 11's.

### 9.2 No feature flag that can stop a bill

See §0.1. If a future plan needs to limit billing — a per-bill quota, a
read-only mode, anything of that shape — it is a product decision that has to be
argued on its own, and it starts by changing `Feature` in `crates/mb-license`,
which fails a test by design.
