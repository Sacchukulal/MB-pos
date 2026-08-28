#!/usr/bin/env node
/**
 * The acceptance gate for the cloud round: a REAL counter against the REAL cloud.
 *
 * Runs against a counter started on an EMPTY data folder with CDP open (the same road
 * `drive.mjs` takes), and a TEST licence made by `MB-backend/tools/make-test-licence.mjs`.
 *
 *   set APPDATA=C:\path\to\empty            (or WEBVIEW2_USER_DATA_FOLDER for a second copy)
 *   target\debug\magic-bill.exe --remote-debugging-port=9222
 *   node scripts/cloud-smoke.mjs --key MB-XXXX-XXXX-XXXX --restaurant <uuid>
 *
 * What it proves, in order:
 *   1. first run → Bring my shop from the cloud with the key (an empty shop comes down: 0 bills)
 *   2. a name and a PIN, three bills sold
 *   3. within two minutes the three bills are in the cloud's `bills` (read with the service key)
 *   4. a staff member added from the cloud side reaches the counter's Staff screen
 *   5. suspend from the admin desk → the next licence check locks Reports, a fourth bill settles
 *   6. resume, so the licence is left as it was found
 *
 * The transfer-to-a-second-APPDATA half is a second run of this script with --second, which
 * expects the first counter's push to be refused afterwards.
 */

import { evaluate } from './drive.mjs';

const args = Object.fromEntries(
  process.argv.slice(2).reduce((out, a, i, all) => {
    if (a.startsWith('--')) out.push([a.slice(2), all[i + 1]?.startsWith('--') ? true : all[i + 1] ?? true]);
    return out;
  }, []),
);
const KEY = args.key;
const RID = args.restaurant;
if (!KEY || !RID) {
  console.error('usage: node scripts/cloud-smoke.mjs --key MB-… --restaurant <uuid> [--second]');
  process.exit(2);
}

/** The cloud, with the service key, from MB-backend/.env — never printed. */
async function cloud() {
  const fs = await import('node:fs');
  const path = await import('node:path');
  const envText = fs.readFileSync(path.resolve('..', 'MB-backend', '.env'), 'utf8');
  const env = Object.fromEntries(
    envText.split(/\r?\n/).filter((l) => l.includes('=') && !l.startsWith('#')).map((l) => [l.slice(0, l.indexOf('=')), l.slice(l.indexOf('=') + 1)]),
  );
  const { createRequire } = await import('node:module');
  const { createClient } = createRequire(path.resolve('..', 'MB-backend', 'package.json'))('@supabase/supabase-js');
  return createClient(env.SUPABASE_URL, env.SUPABASE_SERVICE_ROLE_KEY, { auth: { persistSession: false } });
}

const invoke = async (cmd, params = {}) => {
  const r = await evaluate(`(async () => { try { return { ok: true, out: await window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(params)}) }; } catch (e) { return { ok: false, err: e }; } })()`);
  if (r.error) throw new Error(`${cmd}: ${r.error}`);
  if (!r.value.ok) throw new Error(`${cmd} refused: ${JSON.stringify(r.value.err)}`);
  return r.value.out;
};
const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));
const step = (n, what) => console.log(`\n[${n}] ${what}`);
const check = (cond, what) => {
  if (!cond) {
    console.error(`  ✗ ${what}`);
    process.exit(1);
  }
  console.log(`  ✓ ${what}`);
};

async function waitFor(what, fn, { tries = 24, every = 5000 } = {}) {
  for (let i = 0; i < tries; i++) {
    const got = await fn();
    if (got) return got;
    await sleep(every);
  }
  throw new Error(`gave up waiting for ${what}`);
}

const db = await cloud();

if (args.second) {
  step('T', 'a second computer moves the licence here and brings the shop down');
  const first = await invoke('first_run');
  check(!first.hasShop, 'the second counter starts with no shop');
  const brought = await invoke('restore_from_cloud', { key: KEY, folder: '', moveHere: true });
  check(brought.firstRun.hasShop, `the shop came down: ${brought.says}`);
  check(/[1-9]\d* bills?/.test(brought.says), 'bills came back as bills');
  // The PIN hash came down with the staff, so the counter opens locked: the first counter's PIN
  // must work here.
  await invoke('login', { staffId: 'staff_smoke_owner', pin: '1234' });
  const bills = await invoke('list_bills');
  check(bills.length > 0, `the Bills screen lists ${bills.length} bills that came down, behind the first counter\x27s PIN`);
  console.log('\nNow sell one more bill on the FIRST counter: its push must be refused (Health → Cloud copy says stopped).');
  process.exit(0);
}

step(1, 'first run → Bring my shop from the cloud');
const first = await invoke('first_run');
if (first.hasShop) {
  console.log('  (a shop is already here from an earlier run of this script — carrying on)');
} else {
  check(first.needed, 'the counter is on its first run with no shop');
  const brought = await invoke('restore_from_cloud', { key: KEY, folder: '', moveHere: false });
  check(brought.firstRun.hasShop, `a shop exists now: ${brought.says}`);
}

step(2, 'a name, a PIN, three bills');
await invoke('save_settings', { edits: [{ key: 'store.name', value: 'TEST smoke shop' }] });
const staffId = 'staff_smoke_owner';
await invoke('save_staff_member', { staff: { id: staffId, name: 'Smoke Owner', code: null, roleId: 'role_owner', status: 'active' } });
await invoke('set_staff_pin', { staffId, pin: '1234' });
await invoke('login', { staffId, pin: '1234' }).catch(() => undefined);
await invoke('save_menu_item', {
  edit: { id: 'itm_smoke_tea', name: 'Tea', categoryId: null, price: '20', taxClassId: null, hsn: null, shortCode: null, cost: null, course: null, prepMinutes: null, isOpenPrice: false, isAvailable: true },
});
const sold = [];
for (let i = 0; i < 3; i++) {
  await invoke('cart_clear', { keepType: false });
  await invoke('cart_set_order_type', { orderType: 'Parcel' });
  await invoke('cart_add', { itemId: 'itm_smoke_tea', qty: '1', note: null });
  // settle() — one transaction — and THEN the print; the answer is the bill number.
  sold.push(await invoke('complete_bill', { mode: 'cash' }));
}
check(sold.length === 3, `three bills settled: ${sold.join(', ')}`);

step(3, 'the bills reach the cloud within two minutes');
const localBills = (await invoke('list_bills')).length;
const inCloud = await waitFor('the bills in the cloud', async () => {
  const { data } = await db.from('bills').select('id, grand_total_paise, restore').eq('restaurant_id', RID);
  return data && data.length >= localBills ? data : null;
});
check(inCloud.length >= localBills, `${inCloud.length} bills in the cloud, ${localBills} on the counter`);
check(inCloud.every((b) => b.restore && typeof b.restore === 'object'), 'every bill carries its restore block');
const account = await invoke('account');
check(/Last copied to the cloud/.test(account.cloudCopy), `Account says: ${account.cloudCopy}`);
check(account.restaurantCode.length > 0, `the shop code for phones is shown: ${account.restaurantCode}`);

step(4, 'a staff member added on the cloud side reaches the counter');
const cloudStaffId = `st-cloud-${Date.now().toString(36)}`;
const { error: e1 } = await db.from('staff').insert({ restaurant_id: RID, id: cloudStaffId, name: 'From The Phone', code: 'F' + cloudStaffId.slice(-3).toUpperCase(), can_login_on_phone: true, updated_by: 'phone:smoke' });
check(!e1, `inserted ${cloudStaffId} in the cloud${e1 ? ': ' + e1.message : ''}`);
// The cloud answers a pull five seconds behind the clock (SYNC_PROTOCOL §4), so this polls.
const people = await waitFor('the phone\x27s staff member on the counter', async () => {
  await invoke('pull_from_cloud');
  const list = await invoke('list_staff');
  return list.some((p) => p.id === cloudStaffId && p.name === 'From The Phone') ? list : null;
}, { tries: 10, every: 3000 });
check(people.some((p) => p.id === cloudStaffId), 'the Staff screen shows them (after the pull)');

step(5, 'suspend from the admin desk → Reports lock at the next check, billing does not');
const { error: e2 } = await db.rpc('mb_admin_licence', { restaurant: RID, action: 'suspend', detail: { why: 'smoke' } });
check(!e2, `suspended${e2 ? ': ' + e2.message : ''}`);
const after = await invoke('refresh_licence');
check(after.standing === 'suspended', `standing is now ${after.standing}`);
let reportsRefused = false;
try {
  await invoke('report_list');
} catch (e) {
  reportsRefused = /licence/.test(String(e.message));
}
check(reportsRefused, 'Reports are refused by the licence gate');
await invoke('cart_clear', { keepType: false });
await invoke('cart_set_order_type', { orderType: 'Parcel' });
await invoke('cart_add', { itemId: 'itm_smoke_tea', qty: '1', note: null });
const fourth = await invoke('complete_bill', { mode: 'cash' });
check(Boolean(fourth), `a fourth bill (${fourth}) still settles while suspended`);

step(6, 'resume, so the licence is left as it was found');
const { error: e3 } = await db.rpc('mb_admin_licence', { restaurant: RID, action: 'resume', detail: {} });
check(!e3, `resumed${e3 ? ': ' + e3.message : ''}`);
const back = await invoke('refresh_licence');
check(back.standing === 'fine', `standing is ${back.standing} again`);

const health = await invoke('health');
const cloudRow = health.rows.find((r) => r.id === 'cloud');
console.log(`\nHealth → Cloud copy: ${cloudRow?.says}`);
console.log('\nAll six steps passed. For the transfer half: start a second counter on another empty folder and run with --second.');
