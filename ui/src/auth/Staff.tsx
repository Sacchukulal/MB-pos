/**
 * **The people, and what each of them may do** — audit C2's fix.
 *
 * > *"There are two different staff systems. The POS has its own local staff
 * > list that does nothing… Meanwhile the real staff system lives in the cloud
 * > and is managed only from the Android app. The owner has no idea which one
 * > is real."*
 *
 * There is one list, it is this one, and it is mastered here (D9/D16 — the
 * counter must be able to sign a cashier in during a power cut, and a login is
 * not worth an Edge Function call). P33 makes the cloud a mirror of it.
 *
 * # Nothing here is a control
 *
 * Every button on this screen calls a command that checks `staff.manage` in
 * Rust. The screen not being reachable is a courtesy; `guard::require` is the
 * control, and there is a test that calls the commands directly without it.
 */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  Input,
  Modal,
  Select,
  Table,
  Tabs,
  useToast,
  type Column,
  Page,
  PageHeader,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { PersonView } from '../ipc/generated/PersonView';
import type { RoleView } from '../ipc/generated/RoleView';

import './auth.css';

export function Staff() {
  const [tab, setTab] = useState('people');
  return (
    <Page className="mb-screen">
      <PageHeader
        title="Staff"
        subtitle="Who works here, what each of them may do, and their PIN."
      />
      <Tabs
        tabs={[
          { id: 'people', label: 'People' },
          { id: 'roles', label: 'Roles' },
        ]}
        active={tab}
        onChange={setTab}
      />
      {tab === 'people' ? <People /> : <Roles />}
    </Page>
  );
}

function People() {
  const [people, setPeople] = useState<readonly PersonView[]>([]);
  const [roles, setRoles] = useState<readonly RoleView[]>([]);
  const [editing, setEditing] = useState<PersonView | null>(null);
  const [pinFor, setPinFor] = useState<PersonView | null>(null);
  const toast = useToast();

  const load = useCallback(async () => {
    try {
      setPeople(await call('list_staff'));
      setRoles(await call('list_roles'));
    } catch (cause) {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    }
  }, [toast]);

  useEffect(() => {
    void load();
  }, [load]);

  const columns: Column<PersonView>[] = [
    { key: 'name', header: 'Name', render: (p) => p.name },
    { key: 'code', header: 'Code', render: (p) => p.code ?? '—' },
    { key: 'role', header: 'Role', render: (p) => p.role ?? 'No role yet' },
    {
      key: 'pin',
      header: 'PIN',
      render: (p) =>
        p.hasPin ? <Badge tone="ok">Set</Badge> : <Badge tone="warn">None</Badge>,
    },
    {
      key: 'status',
      header: 'Status',
      // Colour is never the only signal (§2): the word is the signal and the
      // tone is the emphasis.
      render: (p) => (
        <Badge tone={p.status === 'active' ? 'ok' : 'neutral'}>
          {p.status === 'active'
            ? 'Works here'
            : p.status === 'suspended'
              ? 'Suspended'
              : 'Left'}
        </Badge>
      ),
    },
    {
      key: 'do',
      header: '',
      render: (p) => (
        <div className="mb-row">
          <Button small onClick={() => setEditing(p)}>
            Edit
          </Button>
          <Button small variant="quiet" onClick={() => setPinFor(p)}>
            PIN
          </Button>
        </div>
      ),
    },
  ];

  return (
    <>
      <div className="mb-row mb-row--end">
        <Button
          variant="primary"
          onClick={() =>
            setEditing({
              id: `staff_${Date.now()}`,
              name: '',
              code: null,
              role: null,
              status: 'active',
              hasPin: false,
              lockedOut: null,
              permissions: [],
              maxDiscountBp: null,
              maxDiscount: null,
            })
          }
        >
          Add somebody
        </Button>
      </div>

      <Table rows={people} columns={columns} rowKey={(p) => p.id} />

      <p className="mb-muted">
        Nobody is ever deleted. Somebody who leaves is marked as having left, so
        their name stays on the bills they took and in the history.
      </p>

      {editing ? (
        <EditPerson
          person={editing}
          roles={roles}
          onClose={() => setEditing(null)}
          onSaved={(saved) => {
            setPeople(saved);
            setEditing(null);
          }}
        />
      ) : null}

      {pinFor ? (
        <SetPin person={pinFor} onClose={() => setPinFor(null)} onDone={load} />
      ) : null}
    </>
  );
}

function EditPerson({
  person,
  roles,
  onClose,
  onSaved,
}: {
  person: PersonView;
  roles: readonly RoleView[];
  onClose: () => void;
  onSaved: (people: readonly PersonView[]) => void;
}) {
  const [name, setName] = useState(person.name);
  const [code, setCode] = useState(person.code ?? '');
  const [roleId, setRoleId] = useState(
    roles.find((r) => r.name === person.role)?.id ?? '',
  );
  const [status, setStatus] = useState(person.status);
  const toast = useToast();

  const save = async () => {
    try {
      const people = await call('save_staff_member', {
        staff: {
          id: person.id,
          name,
          code: code.trim() === '' ? null : code.trim(),
          roleId: roleId === '' ? null : roleId,
          status,
        },
      });
      onSaved(people);
    } catch (cause) {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    }
  };

  return (
    <Modal open title={person.name === '' ? 'Add somebody' : person.name} onClose={onClose}>
      <Input label="Name" value={name} autoFocus onChange={(e) => setName(e.target.value)} />
      <Input
        label="Staff code"
        hint="Typed at the lock screen instead of tapping a name."
        value={code}
        onChange={(e) => setCode(e.target.value)}
      />
      <Select
        label="Role"
        value={roleId}
        onChange={(e) => setRoleId(e.target.value)}
        options={[
          { value: '', label: 'No role — cannot do anything yet' },
          ...roles.map((r) => ({ value: r.id, label: r.name })),
        ]}
      />
      <Select
        label="Status"
        value={status}
        onChange={(e) => setStatus(e.target.value)}
        options={[
          { value: 'active', label: 'Works here' },
          { value: 'suspended', label: 'Suspended — cannot sign in' },
          { value: 'left', label: 'Has left' },
        ]}
      />
      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" onClick={() => void save()}>
          Save
        </Button>
      </div>
    </Modal>
  );
}

function SetPin({
  person,
  onClose,
  onDone,
}: {
  person: PersonView;
  onClose: () => void;
  onDone: () => void;
}) {
  const [pin, setPin] = useState('');
  const [recovery, setRecovery] = useState<string | null>(null);
  const toast = useToast();

  const save = async (value: string | null) => {
    try {
      const code = await call('set_staff_pin', { staffId: person.id, pin: value });
      if (code) {
        // Shown once, and printed. After this it exists on paper and nowhere
        // else — so the dialog stays open until it is acknowledged.
        setRecovery(code);
        return;
      }
      onDone();
      onClose();
    } catch (cause) {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    }
  };

  if (recovery) {
    return (
      <Modal open title="Write this down" onClose={() => { onDone(); onClose(); }}>
        <p>
          This is the shop&rsquo;s recovery code. It is the way back in if the
          owner forgets their PIN, it is shown here <strong>once</strong>, and it
          cannot be looked up afterwards.
        </p>
        <p className="mb-lock__code">{recovery}</p>
        <Button variant="primary" wide onClick={() => { onDone(); onClose(); }}>
          I have written it down
        </Button>
      </Modal>
    );
  }

  return (
    <Modal open title={`${person.name}'s PIN`} onClose={onClose}>
      <Input
        label="New PIN"
        hint="Six to eight digits. It is stored scrambled and cannot be read back."
        value={pin}
        autoFocus
        inputMode="numeric"
        onChange={(e) => setPin(e.target.value.replace(/[^0-9]/g, ''))}
      />
      <p className="mb-muted">
        Setting the first PIN in this shop locks the screen straight away, so
        the person it belongs to can prove it works before they walk off.
      </p>
      <div className="mb-row mb-row--end">
        {person.hasPin ? (
          <Button variant="danger" onClick={() => void save(null)}>
            Remove the PIN
          </Button>
        ) : null}
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" onClick={() => void save(pin)}>
          Set the PIN
        </Button>
      </div>
    </Modal>
  );
}

function Roles() {
  const [roles, setRoles] = useState<readonly RoleView[]>([]);
  const [permissions, setPermissions] = useState<readonly [string, string][]>([]);
  const [editing, setEditing] = useState<RoleView | null>(null);
  const toast = useToast();

  useEffect(() => {
    void (async () => {
      try {
        setRoles(await call('list_roles'));
        // **The grid is built from the permissions that exist**, never from a
        // list typed into this file — so it can only ever offer a permission
        // the database has a row for (BACKEND-G7).
        setPermissions(await call('list_permissions'));
      } catch (cause) {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      }
    })();
  }, [toast]);

  return (
    <>
      <div className="mb-roles">
        {roles.map((role) => (
          <div key={role.id} className="mb-roles__role">
            <div className="mb-stack">
              <strong>{role.name}</strong>
              <span className="mb-muted">
                {role.permissions.length} of {permissions.length} things allowed
                {role.maxDiscountPercent === null
                  ? ''
                  : ` · up to ${role.maxDiscountPercent} off`}
              </span>
            </div>
            <Button small onClick={() => setEditing(role)}>
              Edit
            </Button>
          </div>
        ))}
      </div>

      {editing ? (
        <EditRole
          role={editing}
          permissions={permissions}
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

function EditRole({
  role,
  permissions,
  onClose,
  onSaved,
}: {
  role: RoleView;
  permissions: readonly [string, string][];
  onClose: () => void;
  onSaved: (roles: readonly RoleView[]) => void;
}) {
  const [name, setName] = useState(role.name);
  const [granted, setGranted] = useState<readonly string[]>(role.permissions);
  // The text Rust formatted, edited as text and sent back as text. Rust parses
  // it — R8, and the reason there is no `/ 100` anywhere in this file.
  const [percent, setPercent] = useState(role.maxDiscountPercent ?? '');
  const toast = useToast();

  const save = async () => {
    try {
      const saved = await call('save_role', {
        role: {
          ...role,
          name,
          permissions: [...granted],
          maxDiscountPercent: percent.trim() === '' ? null : percent,
        },
      });
      onSaved(saved);
    } catch (cause) {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    }
  };

  return (
    <Modal open title={role.name} onClose={onClose} wide>
      <Input label="Name" value={name} onChange={(e) => setName(e.target.value)} />
      <Input
        label="Biggest discount"
        hint="Per cent. Leave it empty for no limit."
        value={percent}
        onChange={(e) => setPercent(e.target.value.replace(/[^0-9.]/g, ''))}
      />
      <div className="mb-permissions">
        {permissions.map(([code, description]) => (
          <Checkbox
            key={code}
            label={description}
            checked={granted.includes(code)}
            onChange={(e) =>
              setGranted(
                e.target.checked
                  ? [...granted, code]
                  : granted.filter((c) => c !== code),
              )
            }
          />
        ))}
      </div>
      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" onClick={() => void save()}>
          Save
        </Button>
      </div>
    </Modal>
  );
}
