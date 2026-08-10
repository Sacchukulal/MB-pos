# The Magic Bill LAN protocol

**This is the contract.** Phase 11 writes the Android client against this
document and nothing else — no access to this repository is assumed, and
anything a phone author needs that is not written here is a defect in this
file.

Built at P19 (the road) and P20 (what travels on it). Decision **D9**.

---

## 0. The one rule that decides everything else

**The counter is the authority. The phone submits an INTENT.**

Only the counter assigns bill and token numbers, computes money, decides what
the kitchen has already been told, prints, and settles.

A phone therefore never sends a price, a total, a tax figure or a discount
amount. Where one would seem useful, the phone asks for the *operation* — "10%
off this line" — and the counter does the arithmetic against the staff member's
limit and answers.

**If a phone does send a money field, it is ignored, not rejected.** Refusing
would break a whole floor of phones one version behind on a Saturday night.

---

## 1. Finding the counter

Two ways, and a client must implement **both**. mDNS is absent on a network
with client isolation, unreliable on cheap consumer routers, and flaky on
several Android builds.

### 1a. mDNS

Service type `_magicbill._tcp.local.`

TXT records:

| key | meaning |
|---|---|
| `id` | the counter's stable server id |
| `v` | protocol version |
| `fp` | the certificate fingerprint, `sha256:` + 64 hex |

Nothing secret is here — mDNS is broadcast to every device on the network,
including the guest phones.

### 1b. The QR (always available)

The counter's **Settings → Phones** screen shows a code containing:

```
magicbill://pair?h=<host>&p=<port>&f=<fingerprint>&t=<token>
```

* `h`, `p` — where to connect.
* `f` — the certificate fingerprint as **base64url of the raw 32 bytes**
  (43 characters). The screen shows the same value as `sha256:` + hex, which is
  what `openssl x509 -fingerprint -sha256` prints; a client may accept either.
* `t` — a **single-use pairing token, valid five minutes**.

The server id is deliberately *not* in the QR — the phone gets it from the
pairing response, and leaving it out keeps the code readable by an old camera.

---

## 2. Security

**TLS with a self-signed certificate that the phone pins.**

1. At pairing the phone learns the fingerprint **from the QR** — that is, from
   a person holding up a screen, not from the network.
2. It stores that fingerprint and, from then on, **refuses any connection whose
   certificate does not match it**. Not "warns". Refuses.
3. It never falls back to the platform trust store. No certificate authority
   can vouch for `192.168.1.7`, and accepting one would be accepting anybody.

The credential is a bearer token, `Authorization: Bearer <device_id>.<secret>`,
sent on every request except `/v1/hello` and `/v1/pair`.

**What this does not cover, stated plainly:** a stranger on the shop's WiFi can
impersonate the counter to a phone that has *never* paired, because that phone
has nothing to compare against. That is why the fingerprint travels in a code a
person holds up, and why pairing needs somebody to press Allow.

---

## 3. Pairing

Two things must both be true. Either alone is not enough.

1. the phone presents a token the counter is **showing right now**; and
2. **a person at the counter presses Allow**, having seen the device's name.

```
POST /v1/pair
{ "name": "Ravi's phone", "platform": "android", "token": "8GF-CVC" }

202 { "request_id": "...", "message": "Waiting for somebody at the counter…" }
```

Then poll:

```
GET /v1/pair/{request_id}

202 { "message": "Waiting for somebody at the counter to allow this phone." }
200 { "device_id": "dev_…", "secret": "…", "server_id": "srv_…" }
400 { "message": "That code has expired or has already been used…" }
```

The secret is returned **once**. Store it in the platform keystore; the counter
keeps only an Argon2 hash of it and cannot tell you again.

The short code may be typed instead of scanned, and is compared case- and
dash-insensitively.

### Refusals

| status | when |
|---|---|
| 400 | the token is wrong, used, or expired |
| 403 | the shop is at its device limit (the sentence has the number in it) |
| 429 | too many attempts — see §7 |

---

## 4. Staying connected

```
GET  /v1/hello                unauthenticated. Server id, version, shop name,
                              fingerprint. Never versioned-refused, so a phone
                              told to upgrade can still ask what it is talking to.
GET  /v1/me                   who this device is, and what its staff member may do
GET  /v1/stream?since=<seq>   the WebSocket
```

### Reconnecting — and the refetch storm this avoids

Every push carries a monotonic `seq`. A phone reconnects with the last one it
saw and is answered with one of:

```json
{ "what": "since", "pushes": [ … ] }
{ "what": "too_far_behind", "newest": 812 }
```

`too_far_behind` is **said explicitly** so the phone asks for a snapshot as a
decision. Fifteen phones each refetching everything after a two-second blip is
how a counter stutters mid-rush.

### The counter's IP changes, or it restarts

The credential is bound to the **server id**, never to an address. Rediscover
and reconnect; do not pair again.

### Version mismatch

Send `x-magicbill-version: <n>`. A mismatch is **426 Upgrade Required** with a
sentence that already says which side needs updating. Show it as-is.

---

## 5. Intents

```json
{
  "id": "<client-generated, unique for ever>",
  "order_id": "<or null, only for open_order>",
  "at": 1786000000000,
  "what": { "do": "add_item", "item_id": "itm_dosa", "qty": "2",
            "note": null, "modifiers": [] }
}
```

`at` is the **phone's** clock in milliseconds. It is used for exactly one
thing — deciding whether a queued intent is too old to apply without asking a
person — and never becomes a business day.

### The operations

| `do` | fields | needs |
|---|---|---|
| `open_order` | `order_type`, `table_id?`, `covers?` | `bill.create` |
| `add_item` | `item_id`, `qty` (decimal string), `note?`, `modifiers[]` | `bill.create` |
| `set_qty` | `line`, `qty` | `bill.create` |
| `void_item` | `line`, `reason` (**compulsory**) | `order.item.void` |
| `set_order_note` | `note?` | `bill.create` |
| `set_covers` | `covers?` | `bill.create` |
| `set_customer` | `customer_id?` | `customers.manage` |
| `request_discount` | `line?`, `percent_bp`, `reason` | `bill.discount.*` |
| `send_to_kitchen` | — | `bill.create` |
| `move_table` | `table_id` | `bill.create` |
| `cancel_order` | `reason` (**compulsory**) | `order.cancel` |
| `request_bill` | — | `bill.create` |

`qty` is a **decimal string**, never a float: `"0.5"`, `"2"`. A quantity
multiplies a price and floating point has no place near money.

### Outcomes

```json
{ "outcome": "ok", "order_id": "…", "total": "240.00",
  "lines": [ { "line": 0, "name": "Masala Dosa", "qty": "2",
               "amount": "240.00", "note": null, "sent_to_kitchen": true } ],
  "token": "7", "note": null }

{ "outcome": "refused", "message": "The kitchen has already made this. Ask …" }

{ "outcome": "held",    "message": "This was typed more than 12 hours ago …",
  "batch_id": "…" }
```

**Every outcome carries a sentence a waiter can read.** Show it. Do not
translate a code into your own wording — the counter's vocabulary is the
product's vocabulary, and two wordings for one event is how a waiter stops
trusting the screen.

All three outcomes are **final**. Do not retry any of them.

---

## 6. Idempotency — read this twice

**Every intent carries a client-generated `id`, and it is the whole design.**

* A retry of the same `id` returns the **original outcome, byte for byte** —
  not "already applied". The phone shows it to a waiter, and one event must
  have one answer.
* The counter records the id in the **same database transaction** as the
  effect. There is no window in which an intent is half-applied.
* So: **when in doubt, retry.** A lost reply is safe to retry. A timeout is
  safe to retry. Re-sending a whole queued batch is safe.

Generate the id before the first attempt and **keep it across app restarts**.
An id regenerated on retry is a duplicate order.

---

## 7. Rate limits

| bucket | burst | refill |
|---|---|---|
| a paired device | 20 | 4 per second |
| `/v1/pair`, per IP | 5 | 1 per 12 seconds |
| `/v1/hello`, per IP | 30 | 5 per second |

A limited request is answered **429 with `Retry-After` in seconds**. Honour it;
do not retry sooner.

---

## 8. Offline

```
POST /v1/batch    { "intents": [ …, …, … ] }
```

Applied **in order**. Idempotency covers the whole batch, so re-sending a batch
that was half-answered is safe and correct.

```json
{ "outcomes": [ ["id1", {"outcome":"ok", …}],
                ["id2", {"outcome":"held", …}] ],
  "says": "38 order changes of 40 went through. 2 are waiting …" }
```

**Per intent, never one status for the batch.** A batch that reports a single
result is a batch whose failures are invisible.

### Held intents

An intent older than **12 hours** is not applied. It waits for somebody at the
counter to say whether it still applies.

v1's failure this prevents: a phone that was offline all evening reconnects
when the shop opens and silently prints yesterday's tickets into a kitchen
making breakfast.

To release one, the phone re-sends it with a **new id and a fresh `at`** — the
release is a new decision, not a retry of the old one.

---

## 9. The catalogue

```
GET /v1/catalogue?version=<what you hold>

304  (unchanged — keep what you have)
200  { "version": "…", "items": [ … ], "tables": [ … ] }
```

The version is a hash of **what a phone can see** — names, prices,
availability — so a shop editing a cost price does not push 400 items to the
floor.

**Sold-out items are sent, marked `is_available: false`**, not omitted. A menu
with a hole in it is a menu where a waiter cannot tell "ran out" from "never
existed". Availability changes are pushed immediately.

---

## 10. The conflicts, and what the counter does about each

Every one of these is decided, not discovered. **The counter always wins, and
the phone is always told.**

### (a) Two waiters open the same table at once

**The second one JOINS the first.** They are serving one party; two orders on
one table is a bill that gets split by accident. The second phone gets the
existing order and the note *"Somebody had already opened this table. You are
both on the same order."*

### (b) A waiter adds items while the cashier is settling

**The counter takes the change** — it is the authority — **and the cashier is
told, never overwritten.** The counter's screen shows *"The floor added 2
Masala Dosa to this table"* and offers to take them in. The cashier's unsaved
payment and discount are untouched either way.

### (c) A waiter voids or shrinks a line the kitchen has already made

**Refused, and sent to the counter.** Throwing away cooked food is a decision
with a cost and it belongs to somebody standing at the till. `set_qty` below
what was cooked is refused for the same reason, in a sentence with the number
in it.

### (d) The cashier moves a table while a phone holds the old one

The counter's table is the truth. The phone's `move_table` applies against the
current order; its own view is corrected by the next push.

### (e) A phone edits an order the counter has finished with

**Refused, with which of the three things happened** — settled, voided or
cancelled — and always with *"Start a new order"*, because the waiter has a
customer in front of them and needs to know what to do, not what went wrong.

### (f) A table the shop no longer has

**Refused in words**: *"That table is not on this shop's floor any more. Pull
down to refresh, and open it again."* Not a database error — a phone holding a
stale catalogue after the counter deleted a table is an ordinary Tuesday.

---

## 11. The kitchen

**Only the counter decides what is new.** `send_to_kitchen` asks; the counter
computes the delta from its own ledger and prints exactly what has not been
sent.

A phone must never compute a delta of its own: on a retry it would print a
second ticket, which is the precise failure the ledger exists to prevent.

Sending again with nothing new is not an error — the counter answers *"The
kitchen already has everything on this order."*

---

## 12. Audit

Every intent is recorded with the **device**, the **staff member**, the time
and the outcome, in the same transaction as the effect.

*"Who cancelled that item?"* is answerable a month later. v1 kept two days.

---

## 13. Two honest edges

**A permission refusal is not in the idempotency ledger.** It is decided before
the transaction, so nothing is written. That is still safe: the answer is a
pure function of who is asking and what they asked, so a retry gets the same
sentence — and it keeps the ledger from filling with rows for a phone that is
repeatedly told no.

**A held intent is not in it either**, for the same reason and with the same
consequence: re-sending a held intent gets held again, until a person releases
it with a new id.
