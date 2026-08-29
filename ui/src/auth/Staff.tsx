/** The people, and what each of them may do. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  freshId,
  Input,
  Modal,
  Page,
  PageHeader,
  Select,
  Table,
  Tabs,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { PersonView } from '../ipc/generated/PersonView';
import type { RoleView } from '../ipc/generated/RoleView';

import { PIN_DIGITS } from './keyboard';
import { Attendance, EmploymentDetails, Leave, Payroll, Salary } from './Employment';
import type { EmployeeView } from '../ipc/generated/EmployeeView';

import './auth.css';

export function Staff() {
  const [tab, setTab] = useState('people');
  const [people, setPeople] = useState<readonly EmployeeView[]>([]);

  // The employment tabs need the list of people to choose between, and the salary tab needs it
  // before anybody has opened People.
  useEffect(() => {
    call('employees')
      .then(setPeople)
      .catch(() => {
        // A person with no staff permission still reaches this screen for their OWN attendance
        // and leave, and an empty list is the honest state for them rather than a toast about
        // permission.
      });
  }, []);

  return (
    <Page className="mb-screen">
      <PageHeader
        title="Staff"
        note="Nobody is ever deleted. Somebody who leaves is marked as having left, so their name stays on their bills and in the history."
      />
      <Tabs
        tabs={[
          { id: 'people', label: 'People' },
          { id: 'attendance', label: 'Attendance' },
          { id: 'leave', label: 'Leave' },
          { id: 'salary', label: 'Salary' },
          { id: 'payroll', label: 'Payroll' },
          { id: 'roles', label: 'Roles' },
        ]}
        active={tab}
        onChange={setTab}
      />
      {tab === 'people' ? <People /> : null}
      {tab === 'attendance' ? <Attendance /> : null}
      {tab === 'leave' ? <Leave /> : null}
      {tab === 'salary' ? <Salary people={people} /> : null}
      {tab === 'payroll' ? <Payroll /> : null}
      {tab === 'roles' ? <Roles /> : null}
    </Page>
  );
}

function People() {
  const [people, setPeople] = useState<readonly PersonView[]>([]);
  const [roles, setRoles] = useState<readonly RoleView[]>([]);
  const [editing, setEditing] = useState<PersonView | null>(null);
  const [pinFor, setPinFor] = useState<PersonView | null>(null);
  /** The employment record behind a person: what they do, and when they left. */
  const [atWork, setAtWork] = useState<PersonView | null>(null);
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
    // The one staff list: what the owner changed on the phone comes down when this screen
    // opens, and the list is read again once it has. Quiet when there is no cloud.
    call('pull_from_cloud')
      .then(() => load())
      .catch(() => undefined);
  }, [load]);

  const columns: Column<PersonView>[] = [
    { key: 'name', header: 'Name', render: (p) => p.name },
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
      // Colour is never the only signal (§2): the word is the signal and the tone is the
      // emphasis.
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
          <Button size="sm" onClick={() => setEditing(p)}>
            Edit
          </Button>
          <Button size="sm" variant="quiet" onClick={() => setPinFor(p)}>
            PIN
          </Button>
          {/*
            What they do, who to call, and the day they left — `save_employee`, which had no
            button at all.
          */}
          <Button size="sm" variant="quiet" onClick={() => setAtWork(p)}>
            At work
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
              id: freshId('staff'),
              name: '',
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

      {atWork ? (
        <EmploymentDetails
          person={atWork}
          onClose={() => setAtWork(null)}
          onDone={() => {
            setAtWork(null);
            toast.show('ok', 'Saved.');
            void load();
          }}
          onFailed={(cause) => {
            if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
          }}
        />
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

/** Setting somebody else's PIN. */
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
  const [again, setAgain] = useState('');
  const [problem, setProblem] = useState('');
  const [recovery, setRecovery] = useState<string | null>(null);
  const toast = useToast();

  const setIt = () => {
    // The same rule Rust holds — `mb_auth::pin::PIN_DIGITS`.
    if (pin.length !== PIN_DIGITS) {
      setProblem(`A PIN is ${PIN_DIGITS} digits.`);
      return;
    }
    if (pin !== again) {
      setProblem('The two PINs are not the same. Type it again.');
      return;
    }
    setProblem('');
    void save(pin);
  };

  const save = async (value: string | null) => {
    try {
      const code = await call('set_staff_pin', { staffId: person.id, pin: value });
      if (code) {
        // Shown once, and printed.
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
        {/* It says "and printed" because it now is. */}
        <p>
          This is the shop&rsquo;s recovery code. It is the way back in if the
          owner forgets their PIN. It is shown here <strong>once</strong> and
          printed on your printer, and it cannot be looked up afterwards — so
          keep the slip somewhere only you can reach.
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
        hint={`${PIN_DIGITS} digits. It is stored scrambled and cannot be read back.`}
        maxLength={PIN_DIGITS}
        value={pin}
        autoFocus
        type="password"
        inputMode="numeric"
        onChange={(e) => setPin(e.target.value.replace(/[^0-9]/g, ''))}
      />
      <Input
        label="The same PIN again"
        maxLength={PIN_DIGITS}
        value={again}
        type="password"
        inputMode="numeric"
        onChange={(e) => setAgain(e.target.value.replace(/[^0-9]/g, ''))}
        onKeyDown={(e) => {
          if (e.key === 'Enter') setIt();
        }}
      />
      {problem ? (
        <p className="mb-lock__problem" role="alert">
          {problem}
        </p>
      ) : null}
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
        <Button variant="primary" onClick={setIt}>
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
        // The grid is built from the permissions that exist, never from a list typed into this
        // file — so it can only ever offer a permission the database has a row for.
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
            <Button size="sm" onClick={() => setEditing(role)}>
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
  // The text Rust formatted, edited as text and sent back as text.
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
