# What P27.5 changed, and what it did not

**The after.** Every row of `UI_DIAGNOSIS.md` is marked here. Screenshots in
`docs/ui/`.

---

## The twenty findings

All twenty are **FIXED**. The table in `UI_DIAGNOSIS.md` carries the mark
against each one, so it is not repeated here; what follows is what was built to
fix them, and then the honest list of what was not done.

---

## What was built

### The navigation moved to the top (D145)

The owner's instruction: *"the verticll left menu bars i didnt like, i wish to
see that in horizontal top side"*.

It shares the title bar rather than taking a row of its own, because at
1366×768 vertical space is what the cart and the table grid are competing for.
Net cost against the old rail: **nothing vertically, and 76px of width back**
for the billing screen.

Six screens sit in the bar — billing, floor, credit, spends, bills, reports,
the counter's day — and seven behind **More**, which names the screen you are
on when you are inside it. Both halves keep their words: §5 rules out bare
icons, and hiding labels to fit thirteen would have been bare icons with extra
steps.

### An icon set (D146)

`kit/Icon.tsx`. 47 icons, one 24×24 grid, stroke 1.75, round cap and join,
never filled, always `currentColor`, sized by token so the text-size setting
moves an icon with its word. Drawn here — no font, no CDN, no package, no
dependency — because this app runs with the internet unplugged and that is a
supported state.

`IconName` is a union type, so a typo is a compile error. The old arrangement
failed silently.

### The contracts (D144, UI_GUIDELINES §8)

Spacing, type, elevation, icons and layout, each decided once and written down
so P28 and P29 cannot drift. The spacing one is the fix for the actual
complaint: **a screen names a JOB, not a number**, and there are five jobs.

### The layout primitives, and one owner for the margin (D147)

`Page`, `PageHeader`, `Toolbar`, `Panel`, `Sections`, `Fields`, `Row`, `Stack`,
`Notice`. The page margin is `.mb-main`'s, in the shell, in one declaration —
which fixed all thirteen screens, including the ones that still do not use
`Page`.

### A second lint

`check-layout.mjs` fails the build on a page margin in a feature file, a
hand-rolled page header, an `<svg>` outside the kit, or a glyph used as an
icon. It is tested against four deliberately broken files, because **a guard
nobody has watched fail is a guard nobody knows works**.

While writing it, a nineteen-session-old flaw in `check-tokens.mjs` turned up:
its escape hatch was unreachable from a CSS file, because CSS has no
line-comment syntax and block comments are blanked before the marker is read.
Both lints now look for the marker in the raw line.

---

## Proved

| | |
|---|---|
| **Rust suite** | 451 + 51 + 227 + … all green, 0 failed |
| **UI suite** | 226 passed, 22 files |
| **clippy** | `--all-targets -D warnings` clean |
| **Lints** | tokens clean (1 documented exception), layout clean, money clean, view names clean |
| **Theme swap** | `theme.test.tsx` still asserts an identical DOM under every theme, now over the new kit |
| **Contrast** | `contrast.test.ts` still green over the new palette |
| **Light and dark** | both looked at on the real machine — `docs/ui/after-billing.png`, `after-billing-light.png` |
| **1366×768** | the reference machine (D12), which is this machine's actual display |
| **40+ tables** | 46 seeded, all visible, no horizontal scroll — `docs/ui/after-floor.png` |
| **Four table states** | free, occupied, warned, late, all on screen at once and told apart by stripe and fill as well as colour |

### A second typography bug, found the same way

`.mb-numeric` carries the mono face so that a column of rupees lines up (§3),
and the `Table` component put that class on the HEADER cell as well as the data
cells. So "On the shelf", "Costs" and "Worth" were set like a terminal, above
columns that were correctly monospaced. **A column header is a label, not a
figure.**

It was invisible until the Stock screen had eighteen rows in it, which is the
whole argument for `look_demo.rs` — and it is the same mistake as the stat
card's note, made in a second place.

### The one product bug found and fixed

`setup::look` had never worked. It read `menu_items` and `is_active` — a table
and a column that have never existed in this product — so the read errored, the
error was swallowed by `unwrap_or_default()`, and **every shop was told it had
done none of its set-up, on the billing screen, for ever.** Nineteen sessions
of green tests never saw it, because nothing had ever rendered that checklist
against a shop with anything in it.

Fixed, the swallow replaced with a logged warning (R3), and covered by
`a_shop_that_has_a_menu_and_a_room_is_not_told_to_add_them` — which fails
against the old SQL.

---

## What I would still not call good

Said plainly, because a report that claims everything is fine is worth nothing.

1. **1920×1080 is not verified.** This machine's display is 1366×768 and there
   is no second monitor, so the wide layout has been reasoned about (fluid
   grids, `auto-fill`, no fixed column counts) and **not seen**. It belongs on
   the owner's checklist.

2. **Four screens got the shell's margin but not a full pass**: Buying, the
   kitchen display, the setup wizard and the lock screen. They are consistent
   and no longer crooked, but their internal composition has not been
   redesigned. Buying in particular still shows three figures as three separate
   `Card`s where a stat row belongs.

3. **Buying was looked at with no suppliers.** `demo_look` now seeds eighteen
   materials with real packs and prices, so Stock has been seen with a full
   table — and that is exactly how the numeric-header bug below turned up. It
   seeds no suppliers or deliveries, so **Buying has still only been seen with
   headers and an empty state.**

4. **The high-contrast theme has not been looked at since the palette
   changed.** Its token block was updated and `contrast.test.ts` passes, but
   nobody has opened it.

5. **Touch has not been tested.** There is no touch monitor here. Targets are
   44px by token and the nav items are 48px tall, but that is arithmetic, not
   a finger.

6. **The set-up strip is still the largest thing on the billing screen** while
   a PIN is unset — deliberately, because that step matters most. It will be
   gone on any shop that sets one, but the owner will see it until they do.

7. **`0 turn(s) today`** on the floor is still a developer plural, written in
   Rust. Left alone under R12 (no product behaviour changes in a look session);
   it is a one-line words fix for P28.

8. **No animation or transition design.** Motion is limited to the 120ms
   colour fades that were already there, and nothing on the billing path
   animates (budget B1). That is safe and it is also plain.
