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

## 2. The look — NOT DONE, and it is the big one

P27.5 redesigned the whole app on 2026-08-15. **You have not seen it.**

Open it and tell me what is wrong. What I already know I have not proved:

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

## 3. The installer — NOT BUILT

`bundle.targets = ["nsis"]` is configured and **has never been run**. Only
`cargo build --release` has ever built this. So:

- [ ] the installer does not exist;
- [ ] **S4** (the download is under 20 MB) is unmeasured;
- [ ] **S5** (install to first printable bill in 3 minutes) is unmeasured;
- [ ] the WebView2 install mode for an offline Windows 10 machine is unconfirmed.

This is a session of its own and it should happen before anybody else installs
this.

---

## 4. Still on P30's list, needing real things

- [ ] A real phone on the shop WiFi, ordering over the LAN.
- [ ] A real PC sleep and wake, mid-service.
- [ ] Two computers on the counter, half an hour apart, both billing (P27's T8
      passes against two processes on one machine; two real machines is the
      honest version).
- [ ] A full day in a real shop.
