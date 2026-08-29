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
person holds up, and why pairing needs the person's own PIN (or, for a shared
tablet, somebody at the counter pressing Allow).

---

## 3. Pairing

One scan. Two things must both be true, and the phone types neither of them:

1. the phone presents a token the counter is **showing right now** — proof that
   somebody is standing in front of the counter's screen; and
2. a person at the counter who may pair phones says **whose phone it is** — a
   name off the staff list, or "shared, nobody's" for a tablet at the pass — and
   presses Allow.

```
POST /v1/pair
{ "name": "Vivo V2443", "platform": "android", "token": "8GF-CVC", "install": "<a stable id for this install of the app>" }

202 { "request_id": "...", "message": "Waiting for somebody at the counter to allow this phone." }
```

The phone polls while the counter decides:

```
GET /v1/pair/{request_id}

202 { "message": "Waiting for somebody at the counter to allow this phone." }
200 { "device_id": "dev_…", "secret": "…", "server_id": "srv_…" }
400 { "message": "That is not the code on the counter's screen now — it may have expired or been used…" }
```

`install` is how one phone stays ONE phone: a phone that signs out and pairs again takes
its own seat back (same device id, new secret) instead of filling the plan with copies of
itself. An old app that sends none is a new phone every time.

The secret is returned **once**. Store it in the platform keystore; the counter
keeps only an Argon2 hash of it and cannot tell you again.

**The code on the counter stays up until somebody closes it, and it changes the
moment a phone uses it.** So one press of "Add phones" pairs the whole floor,
each phone scanning a fresh code — and a screenshot of the screen is worth one
presentation, never two.

The device acts as its person on every intent, with that person's permissions;
a shared device may take an order and nothing else. `GET /v1/me` says which.

The short code may be typed instead of scanned, and is compared case- and
dash-insensitively.

### The phone's cloud login

A paired phone also gets its person's login to the cloud — reports, bills, the
staff desk — from the counter, so nobody types a shop code, a staff code or a
PIN anywhere:

```
POST /v1/cloud-login
Authorization: Bearer <device_id>.<secret>

200 { "session": { access_token, refresh_token, expires_in, expires_at },
      "device_id": "…", "restaurant": { id, name, short_code }, "staff": { id, name } }
403 { "message": "A shared tablet belongs to nobody, so it has no login of its own." }
403 { "message": "The counter cannot reach the cloud right now. Orders still work; reports come once it can." }
```

The counter calls the cloud's `phone-session` function under its own login and
passes the reply through untouched. A refusal here does not touch the pairing:
the phone can take orders, and asks again the next time it opens.

### Leaving

`DELETE /v1/me` under the credential: the phone is leaving. The counter revokes the
device on the spot (the plan's phone count drops by one) and answers 204. The phone
forgets the counter either way.

### Refusals

| status | when |
|---|---|
| 400 | the token is wrong, used, or expired; a person at the counter pressed Refuse |
| 403 | the shop is at its device limit (the sentence has the number in it); a cloud login the counter could not get |
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

Seqs are seeded from the clock at start-up, so they stay monotonic across
counter restarts: a phone holding yesterday's place is told `too_far_behind`
rather than silently filtered to nothing.

`too_far_behind` is **said explicitly** so the phone asks for a snapshot as a
decision. Fifteen phones each refetching everything after a two-second blip is
how a counter stutters mid-rush. The snapshot is `GET /v1/floor` (below) and,
if the catalogue version changed, `GET /v1/catalogue`.

### What a push is

```json
{ "seq": 813, "kind": "floor", "body": { … } }
```

| `kind` | when | `body` |
|---|---|---|
| `floor` | any table or open order changed, by anybody — the cashier, a phone, the kitchen. Bursts are collapsed into one push. | `{ "tables": [ { "id", "state": "free" \| "taken" \| "bill_asked", "order_id" } ], "orders": [ { "order_id", "table_id", "table_label", "order_type", "total", "token", "note", "lines": [ LineView ], "bill_asked": bool, "by": "Ravi" \| null, "by_id": "stf_…" \| null } ] }` — every open order, as an outcome would describe it; `by`/`by_id` name whoever opened it, so a phone can mark its own |
| `catalogue` | the menu, a price, availability, a table or a section changed | `{ "version": "…" }` — compare with what you hold; fetch `/v1/catalogue` if different |

**The floor is the whole floor.** A phone shows every open order — any waiter may look at,
add to or bill any table — and marks the ones its own person opened by `by`. **An order it
holds that is missing from a `floor` body is one the counter has finished with** — settled,
voided, cancelled — and the phone says so on that order rather than guessing which. Unknown
kinds are ignored: a counter one version ahead is an ordinary Tuesday.

While a phone is on `/v1/stream` the counter counts it as live; the counter's own screen shows
that number. A phone that wants to be counted keeps the socket open for as long as it is on
any floor screen.

The catalogue's `tables[].state` is always `free`: what a table is DOING is the floor's, not
the catalogue's, so that a table being taken does not push 400 menu items to every phone.

```
GET /v1/floor                 the same body as a `floor` push, now
```

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
  "sent_at": 1786000000900,
  "what": { "do": "add_item", "item_id": "itm_dosa", "qty": "2",
            "note": null, "modifiers": [] }
}
```

`at` is the **phone's** clock in milliseconds when the person pressed the button;
`sent_at` is the **same clock** at the moment the request left the phone. They
are used for exactly one thing — deciding whether a queued intent is too old to
apply without asking a person — and the counter reads the age as `sent_at − at`,
both from one clock, so a phone that is hours wrong is still exactly right about
how long it held the order. **The counter's clock never judges a phone's**: a
tablet with auto-time off once had every order it sent held as "last night's".
Neither value ever becomes a business day. A phone that omits `sent_at` is
taken as sending now.

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

`request_bill` makes the counter **print the bill** on its bill printer — the same "bill
to the table" paper the counter's own Print bill makes, with the waiter's name on it —
and marks the order `bill_asked` on the floor until it is settled. The waiter carries the
paper over; the money is still taken at the counter.

`qty` is a **decimal string**, never a float: `"0.5"`, `"2"`. A quantity
multiplies a price and floating point has no place near money.

`send_to_kitchen` makes the counter do everything its own send does: the
kitchen-ticket event, the kitchen screen, and the SAME KOT paper through the
print queue — an order from a phone prints exactly like one typed at the counter.

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

**A whole order is ONE batch.** An intent with no `order_id` applies to the order
an `open_order` **earlier in the same batch** opened — the phone cannot know an id
the batch itself creates. So a waiter's whole order is `[open_order, add_item…,
send_to_kitchen]`, one round trip; a replay re-runs the open with the same intent
id, gets the original outcome and so the same order — the adds land once, on it.
This is also why a phone must NOT open an order when a table is merely tapped:
nothing reaches the counter until the person sends, so an abandoned screen leaves
no 0.00 ghost on the floor.

```json
{ "outcomes": [ ["id1", {"outcome":"ok", …}],
                ["id2", {"outcome":"held", …}] ],
  "says": "6 items sent to the kitchen. 1 change is waiting for somebody at the counter …" }
```

**Per intent, never one status for the batch.** A batch that reports a single
result is a batch whose failures are invisible. `says` is what a waiter hears:
the last thing the counter did in their words ("6 items sent to the kitchen.",
"The bill for table 5 is printing at the counter."), then — only if some did
not go — how many are held or refused. Never a count of "order changes".

### Held intents

An intent that was **typed more than 12 hours before it was sent** (`sent_at −
at`, the phone's own clock) is not applied. It waits for somebody at the
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

`items[].category` is the category's **name** ("Tiffin"), never an id — a waiter reads it on
the sheet, and an id on a phone screen was a bug this line exists to prevent.

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

---

## 14. A second till — P27

Everything above was written for a phone. A second **till** speaks the same
protocol over the same TLS with the same pinned certificate and the same
credential, and it is a paired device with a role — **not a second register.**

### 14.1 What a till is, and what it is not

A till has its **own database**. It has the menu, the tax rules, its own
printer, its own drawer and the whole of `mb-core`, so it bills, prints, takes
cash and gives change without asking anybody. A phone has none of that and asks
for everything.

That difference is the whole design. A phone sends **requests**; a till sends
**facts** (D136) — and it sends requests only for the one thing it does not own,
which is the floor (D137).

### 14.2 The roles

One till in a shop is the **main till** (the master). It holds the floor, it is
the shop's book of record, and it is the machine the others forward to. Every
other till is a **secondary**.

The role is a person's decision and never an election (D139). Moving it clears
every master and sets one in a single transaction, so there is never an instant
when two answer as master or none does. The old master is not consulted; it
sees a later `master_since` than its own the next time it opens and stands down.

### 14.3 Joining

Exactly P19's pairing, §3 above, with two differences:

* `platform` is `"till"`. The pairing panel shows it, so the person pressing
  Allow can see that a computer and not a phone is joining.
* the licence is asked `till_room` instead of `device_limit` (D141). They are
  different lines on a plan, so a shop out of phones can still add the counter
  it paid for.

The joining till fetches `/v1/hello` over a connection that trusts nothing,
checks the certificate's fingerprint against the one on the QR a person is
holding, and only then pins it. A stranger answering on that address hands over
a certificate whose fingerprint does not match, and the join is refused with
*"That is not the till on the code."*

The credential and the pinned certificate are written **beside the config**,
never into the database: a backup is restored onto other machines (D27), and one
that carried a terminal's identity would give a shop two tills claiming to be
one.

### 14.4 `POST /v1/forward` — the facts

    {
      "terminal_id":    "term_1755261900123",
      "terminal_name":  "Counter 2",
      "series_prefix":  "B/",
      "orders":         [ <a whole settled, voided or cancelled order> ]
    }

Answered with a receipt:

    {
      "stored":  [["ord_abc", true], ["ord_def", true]],
      "refused": [],
      "says":    "2 bills stored."
    }

Four things are true of it and they are what make it safe:

1. **It is idempotent on each order's id** — `applied_events`, in the same
   transaction as the effect. A repeat is a **success**, which is what lets a
   secondary retry for ever without keeping track of what it has already sent.
2. **Order does not matter.** Bills are independent facts.
3. **Only what is FINISHED travels.** A draft is not a fact: it lives on the
   till that is typing it, and if that till never comes back nobody was charged.
4. **Nothing leaves the sender's queue but a confirmed apply.** Not a timeout,
   not a restart, not a person.

The till describes itself in every batch because the main till may never have
heard from it — a forwarded bill points at a terminal, and the day close per
drawer needs that row to exist. It is also the only place D135 can be checked:
the uniqueness that stops two tills sharing a bill number is shop-wide, and only
the main till sees the whole shop. **A prefix clash refuses the whole batch**,
with the sentence naming the other till, because storing half of it under a
colliding series is the worse answer.

### 14.5 `POST /v1/intent` — the floor

Unchanged, and used by a secondary for exactly the same reasons a phone uses it.
Tables and open orders belong to the main till (D137).

### 14.6 The conflicts between two tills

Section 10's cases, answered by the same code, with one difference: **the
sentence names the till.** *"At the counter"* is not an answer when there are
two counters and the person reading it is standing at one of them.

| What happened | What the loser reads |
|---|---|
| Two tills open the same table | *"This table is already open on Counter 2. You are both on the same order."* — they JOIN it; see 10(a), and D137 for why refusing would be worse |
| One till settles while another adds | *"That bill has already been paid on Counter 1. Start a new order for anything else."* |
| A till goes offline mid-order | Nothing — its order was a draft on its own machine and was never the main till's, so there is nothing to reconcile |
| The main till disappears | *"The main till is off. This till can take counter and parcel bills — table service needs the main till."* |

A one-till shop reads the old sentences unchanged: with one till there is no
name worth saying, so it still says *"at the counter"*, which is where it
happened.

### 14.7 Numbers never travel

**No till ever asks another for a number.** Every till issues out of its own
series — `A/0001`, `B/0001` — so there is no allocation message, no block, no
top-up and no reservation in this protocol, and there is nothing here that a
settle waits for (D135).

That is why a secondary bills at full speed with the main till switched off, and
why R9's two seconds are a bookkeeping budget rather than a billing one.

---

## 15. The owner's remote interface — P28, scope 9.13

The owner is usually **not at the counter**. Everything about employment —
hiring, permissions, salary, advances, approving leave, correcting attendance —
has to be reachable from their phone, and later from the cloud.

**This section is the contract the Android session builds against.** The
service and the protocol are P28's; the phone SCREENS are Phase 11. Writing the
protocol later would have meant changing the service.

### 15.1 The rule that decides everything here

> **A phone is a screen. It is never an authority.** (D9)

Every command below is **permission-checked on the counter**, in
`guard::COMMAND_ACCESS`, in exactly the same table the counter's own screens go
through. There is no second path, no "trusted device" flag, and no permission
that only the phone's UI enforces. `employment_tests::
every_employment_command_refuses_somebody_without_the_permission` calls every
one of them directly, without the permission, and asserts the refusal — that is
the test this whole section rests on.

A device is bound to a **person** at pairing (§3), so the actor on every command
is the person whose phone it is. It is not a parameter and cannot be one.

### 15.2 The command set

Same names, same arguments and same shapes as the counter's own IPC — see
`src-tauri/src/employment.rs`. Carried over the transport in §5's envelope, and
**idempotent on the intent id** (§6, D82) exactly like an order intent: a phone
on a bad connection retries, and a retried advance must not hand over the money
twice.

| command | needs | what it does |
|---|---|---|
| `employees` | `staff.manage` | the people, with the employment record |
| `save_employee` | `staff.manage` | designation, department, ID reference, and the leaving date |
| `attendance` | *signed in* | your own hours; **anybody else's needs `attendance.mark`** |
| `clock_in` / `clock_out` | *signed in* | your own row, and only ever your own |
| `correct_attendance` | `attendance.correct` | change a clock-in or clock-out. **Never your own row** — a rule no permission can express, enforced in the command |
| `save_roster` | `attendance.mark` | who is expected, and when |
| `leave` | *signed in* | your own balance and requests; anybody else's needs `leave.approve` |
| `request_leave` | *signed in* | ask. A manager may ask on somebody's behalf, and the audit row says who did which |
| `decide_leave` | `leave.approve` | approve or reject. **A rejection carries a reason** |
| `adjust_leave` | `leave.approve` | grant an entitlement, or correct a balance with a reason |
| `salary` | `salary.view` | one person's salary history and advances |
| `save_salary` | `salary.manage` | a NEW effective-dated row, never an edit |
| `give_advance` | `salary.manage` | money out of the drawer today |
| `payroll_runs` / `payroll` | `salary.view` | the runs, and one run's lines |
| `compute_payroll` | `salary.manage` | computes a DRAFT. Moves no money |
| `edit_payroll_line` | `salary.manage` | change one figure before approving. The line is marked `edited` |
| `approve_payroll` | `salary.manage` | **where money leaves the shop** |
| `reverse_payroll` | `salary.manage` | a correction is a state, not a delete (D47) |
| `staff_cost` | `salary.view` | wages as a percentage of what the shop took |

### 15.3 Self-service, and why it cannot be a permission

Three of those say *signed in* rather than naming a permission, and the reason
is that **the rule depends on WHOSE row is being asked for**: your own
attendance is yours, and the same command against somebody else's is a refusal.

`Access::SignedIn` is what that is called in `guard.rs`. It is deliberately not
`Access::Public` — public means *works on the lock screen*, and none of these
do. The command itself compares the asked-for id against the session and
refuses; `employment_tests::self_service_cannot_read_somebody_elses_anything`
is the test.

**A screen that merely declines to draw a row has still been sent it.** That is
why this is here and not in React or in Kotlin.

### 15.4 What is NOT here

* **No phone screens.** Phase 11.
* **No cloud.** The transport is the shop's own WiFi (D9). The cloud arrives in
  P31–P35 and reuses this command set rather than replacing it.
* **No statutory payroll** (PF, ESI, TDS). Scope 9.17 is pending the owner's
  decision; see §15 of `FEATURE_SCOPE.md`.
