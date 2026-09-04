# What only you can check

Some things no test can answer, because answering them means looking at paper,
listening for a drawer, or holding a phone up to a screen. This is that list.

P30 asks for this document as a deliverable. It is started here at P27.5
because the printer half of it could finally be run — you had the TVSE plugged
in on 2026-08-15 and said to test it.

---

## 1. The printer — PART DONE

### What was done on 2026-08-15, around 23:45

The TVSE RP3200 Lite was found on USB001, added to a demo shop as the default
80 mm bill printer with the drawer kick enabled, and driven from the app.

| | |
|---|---|
| **Test print — sample bill** | went through the app's queue, reached the Windows spooler, and the queue says **Printed** |
| **Bill 0042** (Table A7, ₹609.00, cash) | settled through the real billing path and printed. Queue says **Printed** |

**This is the first time this product has printed on real hardware.** The
software path works end to end: a bill in the cart, through Rust's document
builder, through the raster engine, through the Windows spooler, to the TVSE.

### What I could not check, and why

I can drive the printer. **I cannot see the paper.** Everything below is
looking, listening or scanning, so it is yours. There should be two slips in
the printer now.

### The checklist — P07's, against the two slips in the tray

Take the **sample bill** first. It has a ruler printed across it on purpose.

- [ ] **Both ends of the ruler sit at the edges of the paper.** If they do not,
      the paper width setting is wrong — it is set to 80 mm.
- [ ] **The ruler's marks line up with the columns above them.**
- [ ] **The cut lands BELOW the last line**, not through it.
- [ ] **The amounts sit hard against the right-hand edge**, and the column of
      them is straight.

Now the **real bill (0042)**:

- [ ] Your shop's name, address, GSTIN and FSSAI are at the top and readable.
- [ ] The items, quantities and rates are right.
- [ ] Taxable value, CGST and SGST are broken out — not one lumped "GST" line.
- [ ] The total on the paper equals the total that was on the screen: **609.00**
- [ ] The printed lines add up to the printed total. (Add them by hand once.
      This is the one thing worth doing with a calculator, ever.)

Then, if you want to go further:

- [ ] **THE OFFSET.** If the slip is not centred: Settings → Printers → nudge
      1 mm and print another sample. Confirm the whole slip moves and nothing
      falls off the right edge. **Then close the app, open it again, and
      confirm the nudge is still there.**
- [ ] **THE TWO ENGINES.** Settings → Printers → Change → engine. Print the
      same bill as *Picture* and then as *Printer font*. They should be the
      same bill. Any difference is worth telling me about.
- [ ] **THE CUT.** A partial cut leaves a tab. Confirm the receipt does not
      fall on the floor, and does not need two hands to tear.
- [ ] **THE CASH DRAWER**, if you have one plugged into the printer:
      - a cash bill opens it;
      - a card bill does **not**;
      - a **REPRINT never does**, ever. This is the cash-control one and it is
        the one worth testing twice.
      - If it never opens at all, try pin 5 instead of pin 2 in Settings.
- [ ] **THE UPI QR.** Turn the QR on in Settings → The bill, print a bill, and
      scan it with your phone. If your printer has no QR encoder the payload
      prints as text instead — **that is correct**, and it means `native_qr`
      should be off for this printer.
- [ ] **58 mm paper**, only if any shop you sell to will use it.

**A logo cannot be tested yet** — the conversion to one-bit happens when you
upload it, and nothing has been uploaded.

---

## 2. The look — YOU SAW IT ON 2026-08-16 AND REJECTED IT

P27.5 redesigned the whole app on 2026-08-15. You installed it on a second
computer the next day and said:

> *"there has so much of annoying bugs and so many things u never looked at
> rwality… the entire page is covered by that annoying notifications of no
> account, no shop, not set pin, and that 6 startup steps with eating entire
> page… big buttons without proper styling eating so much spaces
> unnessorily, even first installation also totaly confused after installing
> where to go, what i have to do."*

**P30.5 is the answer** — see `PROMPTS/P30.5-first-run-and-the-look.txt` and
DECISIONS.md D155/D156. Most of what you hit was not the design: there was no
way to create a shop at all, so every screen failed at once; the licence
banner was stretching to 230 pixels on every page; and the window was asking
for the whole screen, so the bottom of it sat under your taskbar.

**Please install the new one and go through it again.** Then tell me about
the part I did NOT change:

- [ ] **The button sizes.** You said they eat too much space. I fixed every
      place a screenshot showed space being wasted, but I did not shrink the
      buttons themselves — that is your taste, not a bug. Say "a bit smaller"
      or "much smaller" and it is one file, and nothing can break.

What I still have not proved:

- [ ] **1920×1080.** This machine's display is 1366×768, so the wide layout has
      been reasoned about and never seen. If you have a bigger monitor, open it
      there first.
- [ ] **Touch.** No touch monitor here. Every target is 44px by token and the
      navigation is 48px tall, but that is arithmetic, not a finger.
- [ ] **The high-contrast theme.** Its colours were updated and the contrast
      test passes; nobody has opened it.
- [ ] **The navigation split.** Six screens are across the top (Billing, Floor,
      Credit, Spends, Bills, Reports) and seven are behind **More**. If you
      open one of the seven every day, tell me and I will move it.
- [ ] **The accent colour.** It is my choice, and it is one line to change.

Screenshots of before and after are in `docs/ui/`.

---

## 3. The installer — **BUILT at P30, REBUILT at P30.5, and it fits**

It had never been run. It has now, twice: 6.78 MB at P30, **6.80 MB at
P30.5**. Budget S4 (under 20 MB) is met with room to spare either way.

**You ran the P30 one and it is what P30.5 came out of** — you found there was
no way to create a shop, so install the P30.5 build instead. If the P30 build
is still on that computer, uninstall it **and delete
`C:\Users\<you>\AppData\Roaming\MagicBill\`** first, or you will not see the
new first-run screen: it only appears when there is no shop.

```
cargo tauri build
  → target\release\bundle\nsis\Magic Bill_0.1.0_x64-setup.exe
```

| | |
|---|---|
| **the installer** | **6.78 MB** |
| **budget S4** — the download is under 20 MB | **MET**, with room to spare |
| the binary inside it | 23.65 MB (NSIS compresses it) |
| the front end | 455 KB of JavaScript, 73 KB of CSS, 124 KB gzipped |

**What is still yours to check**, because it needs a second computer:

- [ ] **Run the installer on a machine that is not this one.** Ideally the
      oldest Windows 10 PC you can find, with no developer tools on it.
- [ ] **S5 — install to a printed bill inside three minutes.** Time it from
      double-clicking the setup to holding a printed bill. If it takes longer,
      tell me where the time went; that is a budget and it is missable.
- [ ] **WebView2 on an offline machine.** The installer is set to download it
      if Windows does not have it, which needs internet. A shop being set up in
      a back room with no WiFi is the case to try. If it fails, the fix is a
      different bundle mode and it is a one-line change.
- [ ] **Windows SmartScreen.** The build is unsigned, so Windows will warn the
      first person who runs it. Signing needs a certificate you buy; tell me
      when you have one.
- [ ] Uninstall it afterwards and check the shop's data is **still there**. It
      should be: the database lives beside the config, not in the program
      folder.

---

## 4. Still on P30's list, needing real things

- [ ] A real phone on the shop WiFi, ordering over the LAN.
- [ ] A real PC sleep and wake, mid-service.
- [ ] Two computers on the counter, half an hour apart, both billing (P27's T8
      passes against two processes on one machine; two real machines is the
      honest version).
- [ ] A full day in a real shop.

---

# P29's devices — NONE OF THESE HAS BEEN PLUGGED IN

Added 2026-08-16.

P29 built the software for a barcode scanner, a weighing scale, a customer
display, a label printer and a payment machine. **This computer has a printer
and nothing else.** So what is claimed is exactly this:

| | |
|---|---|
| the parsing, the timing rule, the stability rule | proved, with tests |
| the failure path — absent, unplugged, silent | proved, with tests |
| that any of it works with real hardware | **not claimed** |

Every one of these devices is optional and none of them can stop a bill: a
counter with nothing plugged in bills exactly as it did before P29, and that is
a test (T1). What is below is what only you can check.

**Where to look in the app:** More → **Devices**. It lists what is set up, has a
Test button per device, and shows **the raw data a device is sending** — which
is what a dealer needs to configure a brand nobody here has ever seen. The
settings themselves are in Settings → **Devices**.

---

## 5. The barcode scanner

**What to buy:** any USB "keyboard wedge" scanner (₹1,200 upwards). It plugs in
as a keyboard; there is no driver and nothing to configure in Windows.

**Set up first:** put a code into an item — Menu → the item → the code field.
The scanner types that code, so anything you can type there, it can scan.

- [ ] Open Billing, point the scanner at a packet, pull the trigger. **The item
      is added to the bill**, at quantity 1, without touching the keyboard.
- [ ] **Type a nine-letter word into the search box as fast as you can and press
      Enter.** It must search the menu, **not** be treated as a barcode. This is
      the one that matters: missing a scan costs one re-scan, but reading your
      typing as a scan throws away what you typed.
- [ ] Scan something with no code in the menu. It should say *"Nothing on this
      counter has the code …"* and tell you where to add it — not fail silently.
- [ ] Turn on Settings → The bill → **Print the bill number as a barcode**,
      print a bill, then scan the barcode on that paper. It should find the
      bill and say so.
- [ ] If your scanner is too fast or too slow to be told from typing, the two
      timing settings are in Settings → Devices. Tell me what happened and I
      will change the defaults.

---

## 6. The weighing scale

**What to buy:** any counter scale with a serial (RS-232) or USB-serial output —
the kind a sweet shop or a meat counter uses. Ask the dealer for the **baud
rate** and whether it sends continuously.

**Set up first:** Settings → Devices → the port (COM3, COM4…), the speed, and
the shape it talks in.

- [ ] Open the device screen and press **Test it** on the scale. If the shape is
      wrong you will see **the raw bytes it is sending** — send me that line and
      the right shape becomes a one-line change.
- [ ] Choose **"Show me what it is sending"** and press Test with something on
      the pan. That is the mode that exists so a scale nobody here has ever seen
      can be set up without waiting for us.
- [ ] Put an item on the bill, open the quantity box and press **Weigh**. The
      weight should arrive as the quantity.
- [ ] **Put a bag down and press Weigh while it is still bouncing.** It must say
      *"still settling"* and take nothing. A weight grabbed on the way up is a
      customer undercharged for ever.
- [ ] Unplug the scale and press Weigh. It must say so in a sentence and **the
      bill must still complete**.

**Weight-encoded labels** (the sticker a scale prints, with the weight inside
the barcode): the parsing is built and tested, and where each digit sits is a
setting because every brand differs. If you use them, send me one label and I
will tell you the four numbers to type in.

---

## 7. The customer display

**What to buy:** either a spare monitor on the second HDMI port (cheapest and
best), or a serial VFD pole display.

**Set up first:** Settings → Devices → **Show the customer their bill**. For a
pole display, add its COM port too.

- [ ] Turn it on. A second window opens showing your shop's name.
- [ ] Add items to a bill and watch the second screen: the lines and the total
      should follow along.
- [ ] **THE ONE THAT MATTERS.** Type a whole bill, start to finish, without
      touching the mouse. **Your typing must never jump out of the search box.**
      If you have to click back into it even once, tell me and the feature comes
      out — a display that steals the keyboard will be unplugged by Friday, and
      it is better not shipped than shipped.
- [ ] Drag the second window onto the second monitor and restart the app. It
      should come back where you left it.
- [ ] Unplug the second monitor while billing. Nothing should happen to the
      billing screen.

---

## 8. The label printer

**What to buy:** any small thermal label printer that Windows sees as a printer.

**Set up first:** add it in Settings → Printers, then put its id into
Settings → Devices → **Parcel labels print on**.

- [ ] A **Label** button appears on the billing screen. It appears only when a
      label printer is set up — that is deliberate.
- [ ] Press it with a parcel on the bill. Check the sticker: the item, the
      quantity, the table or token, and your shop's name.
- [ ] Switch the label printer off and press it again. The job should sit in the
      print queue and print when the printer comes back — the same as a bill.

---

## 9. The payment machine

**Nothing here is signed up for, and that is on purpose** — it is your
commercial decision (FEATURE_SCOPE §15, X17).

What ships today is the honest version: when you press **UPI** or **Card**, you
can type the reference, and the payment is recorded as **not confirmed** until
somebody says the money arrived.

- [ ] Take a UPI payment and settle the bill.
- [ ] Open **More → Day close**. There is a **Not confirmed yet** list
      with that payment on it, its reference, and an **It arrived** button.
- [ ] Check your bank app, press **It arrived**, and watch it leave the list.
- [ ] At the end of a real evening, look at that list before you close. **That
      is the whole feature**: a shop cannot chase what it cannot list.

**What is still yours to decide:**

- **A UPI aggregator** (Razorpay, Cashfree, PhonePe and others) would let the
  till confirm a payment by itself. Each one charges per transaction and needs a
  business account and KYC. Tell me which and it becomes a short prompt — the
  billing code does not change.
- **A card terminal** that takes a pushed amount needs the acquiring bank's own
  SDK, which differs per bank. Tell me which bank's machine you have.

---

## 9a. The day close — only a real evening tests it

- [ ] **More → Day close.** Everything for the night is on it: what today came
      to, the notes and coins, and the one button that finishes it.
- [ ] Type the drawer count, press **Close today**, and read what it asks. It
      names the difference before you agree to anything.
- [ ] Do it again on another night **without** typing a count. It closes just the
      same and says so — the count is optional and always was.
- [ ] With today closed, try to take a bill. The counter refuses and the bell
      says *"Today is closed"* with a button straight back here. Try an expense,
      a credit repayment and a supplier payment: all four refuse with the same
      sentence, which is the point.
- [ ] Press **Open today again**, give a reason, and bill. The reason is in
      History against your name.
- [ ] **Settings → The day → Close the day every day.** Switch it off. The Day
      close screen keeps every figure and the drawer count, and has nothing to
      press. Nothing is refused anywhere. Switch it back on and the same days
      lock again.
- [ ] **Your day starts at** on the same page. Set it to 06:00, print a bill at
      05:30 the next morning, and check it lands on the night before.

---

## 10. Delivery — no hardware, but only you can judge it

- [ ] Take a delivery order, give it to a rider, send it out and mark it
      arrived. Then take the cash off the rider on the Delivery screen.
- [ ] **Watch the drawer figure while a rider is out.** The day close should
      say *"Still with the riders"* and take that money out of what it expects.
      Put it back with the handback and check the drawer figure returns.
- [ ] Send one out and mark it **Did not arrive** with a reason. Check the
      reason is on the screen the next day.
- [ ] Print a **delivery slip** and look at the paper: the address big enough to
      read at a gate in the dark, and either **COLLECT ₹640** or **PAID** — a
      rider who cannot tell those apart asks for money that has already been
      paid.

---

## 11. The recovery code — RUN, and what is left is the paper

The sign-in screen was rebuilt on **2026-08-22** after you found three things
wrong with it on a real install: the pad took eight digits instead of four,
*Somebody else* rubbed out one digit per press instead of leaving, and the
forgotten-PIN flow could be started and never finished.

All three are fixed. The forgotten-PIN flow now walks four steps — the code,
who it is for, the new PIN on a keypad, and the same PIN again — and the whole
of it was driven in the real window on a **fresh shop**, with a real recovery
code: the code was accepted, the new PIN was typed on the keypad, it signed in
afterwards, and the old PIN was refused. So the software half is checked.

**One thing was also found and fixed while going through this, and it is the
part only you can now confirm.** The recovery code was supposed to print. The
program said so on the screen, `mb_auth::recovery` said so in the code, and
your own history says *"New recovery code printed"* — and nothing had ever put
it on a printer. If you had closed that dialog without a pen to hand, the code
was gone. It queues a slip now, both when the code is first issued and when it
is spent, and the queue was checked. **I cannot see the paper.**

- [ ] **Set a PIN for somebody who manages staff on a shop that has no recovery
      code yet** — or watch it on your next fresh install — and check a slip
      comes out of the printer.
- [ ] Read the slip. The code should be the **biggest thing on it**, in two
      groups of five, with the shop's name and the date. It must **not** name
      the person whose PIN it set.
- [ ] Check both halves of the code are inside the paper edges on your 80 mm
      roll.

### Your own shop's code — I did not touch it

Using a code retires it and issues a new one, so testing the flow with your
real slip would have silently invalidated the paper in your drawer. On your
shop I ran a deliberately wrong code instead, and the program answered
correctly: *"That is not this shop's recovery code."*

- [ ] Find the recovery slip that printed when this shop was set up. If you
      cannot find it, **say so** rather than testing it — without a code the
      only way back into a shop is a restored backup, and there is no hurry to
      discover that on a Saturday. A manager can always set a fresh PIN for
      anybody else from the Staff screen; the code is only for when nobody can
      get in at all.
