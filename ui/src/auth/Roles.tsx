/** The roles: what each one may do, box by box, section by section. */

import { useEffect, useRef, useState } from 'react';

import { Button, Checkbox, freshId, Icon, Input, Modal, MoneyInput, Toolbar, useToast } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { PermissionGroupView } from '../ipc/generated/PermissionGroupView';
import type { RoleView } from '../ipc/generated/RoleView';

export function Roles() {
  const [roles, setRoles] = useState<readonly RoleView[]>([]);
  const [groups, setGroups] = useState<readonly PermissionGroupView[]>([]);
  const [editing, setEditing] = useState<RoleView | null>(null);
  const [ready, setReady] = useState(false);
  const toast = useToast();

  useEffect(() => {
    void (async () => {
      try {
        setRoles(await call('list_roles'));
        // The sections, their order and their words are Rust's; the database says which boxes
        // exist, so the screen can only ever offer a permission there is a row for.
        setGroups(await call('list_permissions'));
        setReady(true);
      } catch (cause) {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      }
    })();
  }, [toast]);

  // "12 of 38": counted against the boxes on offer, so a permission that is no longer offered
  // does not make a role look bigger than the screen can show.
  const offered = new Set(groups.flatMap((g) => g.permissions.map((p) => p.code)));

  return (
    <>
      <Toolbar
        end={
          <Button
            variant="primary"
            disabled={!ready}
            onClick={() => setEditing({
              id: freshId('role'),
              name: '',
              isBuiltin: false,
              isOwner: false,
              permissions: [],
              maxDiscountPercent: '0%',
              maxDiscount: null,
              maxDiscountRupees: '0',
            })}
          >
            Add role
          </Button>
        }
      />
      <div className="mb-roles">
        {roles.map((role) => (
          <div key={role.id} className="mb-roles__role">
            <div className="mb-stack">
              <strong>{role.name}</strong>
              <span className="mb-muted">{describe(role, offered)}</span>
            </div>
            {role.isOwner ? (
              // Everything, always: the one role nobody can lock the shop out of.
              <span className="mb-muted mb-roles__locked">
                <Icon name="lock" size="sm" />
                Cannot be changed
              </span>
            ) : (
              <Button size="sm" onClick={() => setEditing(role)}>
                Edit
              </Button>
            )}
          </div>
        ))}
      </div>

      {editing ? (
        <EditRole
          role={editing}
          isNew={!roles.some((role) => role.id === editing.id)}
          groups={groups}
          onClose={() => setEditing(null)}
          onSaved={(saved) => {
            setRoles(saved);
            setEditing(null);
          }}
        />
      ) : null}
    </>
  );
}

/** One line under the role's name. */
function describe(role: RoleView, offered: ReadonlySet<string>): string {
  if (role.isOwner) return 'All permissions';
  const held = role.permissions.filter((code) => offered.has(code)).length;
  const allowed = `${held} of ${offered.size} allowed`;
  // A cap of nothing is "no discounts", not "up to 0%".
  if (role.maxDiscountPercent === '0%') return `${allowed} · no discounts`;
  const caps = [
    role.maxDiscountPercent,
    role.maxDiscount ? `₹${role.maxDiscount.text}` : null,
  ].filter((cap): cap is string => typeof cap === 'string' && cap !== '');
  return caps.length === 0 ? allowed : `${allowed} · discount limits: ${caps.join(' / ')}`;
}

function EditRole({
  role,
  isNew,
  groups,
  onClose,
  onSaved,
}: {
  role: RoleView;
  isNew: boolean;
  groups: readonly PermissionGroupView[];
  onClose: () => void;
  onSaved: (roles: readonly RoleView[]) => void;
}) {
  const [name, setName] = useState(role.name);
  const [granted, setGranted] = useState<ReadonlySet<string>>(new Set(role.permissions));
  // The text Rust formatted, edited as text and sent back as text — Rust does the arithmetic.
  const [percent, setPercent] = useState(role.maxDiscountPercent?.replace('%', '') ?? '');
  const [rupees, setRupees] = useState(role.maxDiscountRupees ?? '');
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const set = (codes: readonly string[], on: boolean) => {
    setGranted((was) => {
      const next = new Set(was);
      for (const code of codes) {
        if (on) next.add(code);
        else next.delete(code);
      }
      return next;
    });
  };

  const save = async () => {
    setBusy(true);
    try {
      const saved = await call('save_role', {
        role: {
          ...role,
          name: name.trim(),
          permissions: [...granted],
          maxDiscountPercent: percent.trim() === '' ? null : percent,
          maxDiscountRupees: rupees.trim() === '' ? null : rupees,
        },
      });
      onSaved(saved);
    } catch (cause) {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open
      title={isNew ? 'Add role' : role.name}
      note="Select individual permissions or a whole section. Help explains any additional effects."
      onClose={onClose}
      wide
      actions={
        <>
          <Button variant="quiet" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={busy || name.trim() === ''} onClick={() => void save()}>
            Save
          </Button>
        </>
      }
    >
      <div className="mb-roles__caps">
        <Input label="Name" value={name} onChange={(e) => setName(e.target.value)} />
        <Input
          label="Percentage discount limit (%)"
          hint="Blank means no limit. Percentage discounts must also stay within the rupee limit."
          value={percent}
          inputMode="decimal"
          onChange={(e) => setPercent(e.target.value.replace(/[^0-9.]/g, ''))}
        />
        <MoneyInput
          label="Discount amount limit (₹)"
          hint="Blank means no limit. Applies to fixed-amount discounts and the value of percentage discounts."
          value={rupees}
          onChange={setRupees}
        />
      </div>

      <div className="mb-permissions">
        {groups.map((group) => (
          <Section key={group.name} group={group} granted={granted} onSet={set} />
        ))}
      </div>
    </Modal>
  );
}

/** One section: a tick for the whole of it, then its boxes. */
function Section({
  group,
  granted,
  onSet,
}: {
  group: PermissionGroupView;
  granted: ReadonlySet<string>;
  onSet: (codes: readonly string[], on: boolean) => void;
}) {
  const codes = group.permissions.map((p) => p.code);
  const on = codes.filter((code) => granted.has(code)).length;
  const all = on === codes.length;
  // The section's own box: ticked, clear, or the half-tick that says "some of these".
  const box = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (box.current) box.current.indeterminate = on > 0 && !all;
  }, [on, all]);

  return (
    <section className="mb-permissions__group" aria-label={group.name}>
      <div className="mb-permissions__head">
        <Checkbox
          ref={box}
          label={group.name}
          checked={all}
          onChange={(e) => onSet(codes, e.target.checked)}
        />
        <span className="mb-permissions__count">
          {on} of {codes.length}
        </span>
      </div>
      <div className="mb-permissions__boxes">
        {group.permissions.map((p) => (
          <Checkbox
            key={p.code}
            label={p.label}
            hint={p.hint}
            checked={granted.has(p.code)}
            onChange={(e) => onSet([p.code], e.target.checked)}
          />
        ))}
      </div>
    </section>
  );
}
