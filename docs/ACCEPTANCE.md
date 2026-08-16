# The ten things that must be true

P30's first job. The master plan says:

> If any of these is false, the rebuild has failed, however good the rest
> looks.

Each one below is answered by a **named, automated test** — not by an opinion
and not by "the session that built it says so". Run any of them by name:

```
cargo test --workspace <name>
```

Where a requirement needs more than one test to be honest, all of them are
listed. Where something is **not** proved, it says so in the same words it
would use if it were, because a checklist that only records successes is a
checklist nobody can act on.

---

## 1. A hard disk failure loses NOTHING

| test | what it proves |
|---|---|
| `t1_a_whole_shop_survives_delete_and_restore` | a whole shop backed up, the database deleted, restored onto a clean profile, every counted table equal |
| `every_table_is_counted_or_named_as_not` | **no table can be added without being backed up or explicitly excluded** — this is the one that stops the list rotting, and it has caught eleven missing tables since P12 |
| `t15_a_backup_carries_the_photographs_and_a_verify_catches_a_damaged_one` | D132 — a backup is the `.db` *plus* the attachments folder *plus* a manifest |
| `t15_a_backup_from_before_this_session_still_restores` | an old single-file backup still restores |
| `t3_a_corrupted_backup_is_caught_and_refused` | a damaged file is refused rather than half-restored |
| `t4_a_failed_restore_rolls_back_to_the_safety_copy` | a restore that fails leaves the shop exactly as it was |
| `t7_a_backup_during_active_billing_is_consistent` | taking one mid-service does not catch half a bill |
| `t6_retention_keeps_the_right_backups_and_never_the_newest` | the newest is never the one deleted |

**Not proved:** a restore onto a genuinely different physical machine. The test
restores onto a clean profile on this one. `OWNER_TESTS.md` carries it.

## 2. A bar can bill legally

| test | what it proves |
|---|---|
| `the_rate_wise_tax_report_equals_what_the_bills_printed` | the summary a CA files from is the same arithmetic as the paper |
| `the_hsn_summary_agrees_with_the_rate_wise_one` | two reports of the same day agree (audit B11) |
| `interstate_is_all_igst` | place of supply decides CGST+SGST or IGST |
| `a_whole_day_reconciles_against_itself` (P30) | one day with 5%, 18% and **non-GST liquor** on the same bill, each its own row |
| `a_service_charge_carries_its_own_rate_into_the_summary` | a charge is taxed at its own rate, not the food's |

## 3. Billing never stops

| test | what it proves |
|---|---|
| `a_shop_bills_with_an_expired_plan` | an expired licence does not stop a sale (D86) |
| `a_shop_with_no_printer_can_still_print` | no printer is a queued job, not a refusal |
| `a_test_print_works_with_no_shop_and_no_printer` | even the diagnostic path |
| `a_shop_with_no_devices_at_all_bills_normally_and_is_not_nagged` | P29 T1 — no scanner, no scale, no display, no label printer |
| `a_scale_that_does_not_answer_says_so_quickly_and_the_bill_still_settles` | P29 T2 — a device that is plugged in and silent |
| `a_declined_card_leaves_the_bill_unsettled_and_says_why` | the ONE case where a payment is refused is the bank saying no, never a device failing |
| `a_clock_that_goes_backwards_does_not_lock_the_counter` | a wrong clock is not a shop that cannot sell |
| `an_offline_deactivate_queues_and_says_the_licence_is_still_held` | no internet |

## 4. Nobody can walk up to the counter and use it

| test | what it proves |
|---|---|
| `every_command_is_classified` | **every single command is classified** — the table cannot go stale |
| `the_public_list_stays_short_and_deliberate` | and "public" cannot quietly grow (D150) |
| `only_the_owner_can_manage_staff_or_restore_a_backup` | the sharp permissions are the owner's |
| `self_service_cannot_read_somebody_elses_anything` | your own row, or a refusal |
| `a_corrupted_hash_is_a_locked_door_not_an_open_one` | a damaged PIN hash refuses rather than admits |
| `a_first_run_is_not_locked_out_of_itself` | …and a shop with no PIN yet can still open |

## 5. A wrong bill can be voided, with a reason, and it shows in reports

| test | what it proves |
|---|---|
| `a_bill_can_be_voided_and_the_days_figures_still_tie` | the day's totals after a void |
| `a_void_reaches_the_control_report_with_its_reason_and_its_person` | audit C4 — who, when, why |
| `a_voided_bill_keeps_its_number_and_its_amounts` | D47 — a void is a STATE, never a deletion |
| `a_big_void_needs_a_second_person` | a large void needs an approver's PIN |
| `a_closed_day_refuses_a_void_until_somebody_opens_it_again` | a locked day is locked |
| `a_whole_day_reconciles_against_itself` (P30) | **and the drawer follows** — a voided cash bill is no longer cash the drawer expects |

## 6. A waiter can take an order with the internet cable unplugged

| test | what it proves |
|---|---|
| `the_same_intent_fifty_times_makes_one_order` | the whole path, on a local network, no cloud — and a retried order does not double |
| `a_certificate_covers_the_lan_and_pins_stably` | the counter's own certificate, pinned |
| `the_counters_figure_wins_over_anything_a_phone_sends` | D9 — the phone asks the counter, and the counter decides |
| `the_counter_decides_the_kitchen_delta_and_a_retry_sends_nothing` | the delta is the counter's, never the phone's |
| `a_phone_cannot_wipe_what_the_cashier_is_typing` | and it cannot take the cart away from the person at the till |

## 7. The printed lines always sum to the printed total

| test | what it proves |
|---|---|
| `every_generated_bill_reconciles_both_ways` | a property test over many generated bills: the printed lines and charges plus the round-off ARE the grand total, and the tax summary reconciles too |
| `t2_golden_files` | a known bill rendered at every paper size against a committed snapshot, so a change to a receipt is read as a diff |
| `every_grouping_of_a_period_sums_to_the_same_gross` | and every way of grouping a period gives one gross |
| `t15_and_t16_a_real_bill_reconciles_in_sql_and_survives_the_round_trip` | the rows behind it agree too |
| `t8_the_printed_tax_summary_sums_to_the_bills_tax` | and the summary block on the paper |

## 8. One business day, everywhere

| test | what it proves |
|---|---|
| `a_bill_after_midnight_appears_on_exactly_one_day_in_every_report` | audit B1, the finding that started the rebuild |
| `a_bill_after_midnight_belongs_to_one_day_on_every_screen` (P30) | the order, its payment row and the report carry the same stamped day |
| `a_shop_can_choose_when_its_day_starts` | 5 a.m. is a setting, not a constant |
| `an_order_takes_its_numbers_from_its_own_business_day` | the counter resets on the day the order belongs to |

## 9. The day can be closed and locked

| test | what it proves |
|---|---|
| `a_drawer_that_matches_closes_without_a_reason` | the ordinary evening |
| `a_short_drawer_cannot_be_closed_without_saying_why` | audit B15 — the reason IS the feature |
| `per_till_drawers_sum_exactly_to_the_shops_day` | D140 — two tills, one shop total |
| `the_float_left_in_the_drawer_is_tomorrows_opening` | tomorrow starts where tonight ended |
| `a_closed_day_refuses_a_void_until_somebody_opens_it_again` | locked means locked |

## 10. Every rule above has a test that fails when broken

The other nine rows are only worth anything if the tests actually bite. Three
were checked by **deliberately breaking the rule** in a scratch copy and
watching the named test go red:

| rule | what was broken | the test that went red |
|---|---|---|
| 8 — one business day | `BusinessDay::of` made to use the calendar date instead of the day rule | `a_bill_after_midnight_appears_on_exactly_one_day_in_every_report` |
| 1 — a backup loses nothing | a table removed from `backup::COUNTED` | `every_table_is_counted_or_named_as_not` |
| 4 — nobody walks up and uses it | one command's row deleted from `guard::COMMAND_ACCESS` | `every_command_is_classified` |

Each was restored immediately afterwards; the point of the exercise is that a
green suite means something.

The same idea is enforced continuously by the guards that fail the BUILD rather
than a test: `check-tokens.mjs`, `check-layout.mjs`, `check-no-money.mjs`,
`check-view-names.mjs`, `schema_rules.rs`'s two-way SCHEMA.md diff, and P30's
own `hygiene_tests.rs`.

---

## What P30 found, and fixed

Two bugs, both invisible from the counter until the moment they cost something:

1. **A parked parcel order could not be reopened from the floor.** The floor's
   "No table" group names a tile by its ORDER id, and `open_table` only ever
   looked for a table — so tapping one found nothing, fell into the "free
   table" branch, and started a NEW empty order whose table was set to an order
   id. That order then failed the foreign key on settle and told the shop its
   data could not be read, while the parked food sat on disk unreachable.
   Caught by `a_restart_mid_service_keeps_the_open_orders`.
2. **`orders.order_type` was never updated after the first save** (found at
   P29): a bill switched to Delivery mid-order settled as a dine-in.

Both now have a test that would have caught them.
