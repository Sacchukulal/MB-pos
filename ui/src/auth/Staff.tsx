/** The people, and what each of them may do. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  Fact,
  Facts,
  Fields,
  freshId,
  Input,
  Modal,
  Page,
  PageHeader,
  PhoneInput,
  SectionHeader,
  Select,
  Stack,
  Table,
  Tabs,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { PersonView } from '../ipc/generated/PersonView';
import type { RoleView } from '../ipc/generated/RoleView';
import type { StaffDetailView } from '../ipc/generated/StaffDetailView';
import type { StaffEdit } from '../ipc/generated/StaffEdit';

import { PIN_DIGITS } from './keyboard';
import { Attendance, Leave, Payroll, Salary } from './Employment';
import { blankPerson, editOf } from './person';
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

const STATUS_WORDS: Record<string, string> = {
  active: 'Works here',
  suspended: 'Suspended',
  left: 'Left',
};

const WORKING_WORDS: Record<string, string> = {
  full_time: 'Full time',
  part_time: 'Part time',
  casual: 'Casual — as needed',
};

function People() {
  const [people, setPeople] = useState<readonly PersonView[]>([]);
  const [roles, setRoles] = useState<readonly RoleView[]>([]);
  /** The card: who is being looked at. */
  const [viewing, setViewing] = useState<string | null>(null);
  /** The one dialog: somebody new, or somebody's record as it is. */
  const [editing, setEditing] = useState<StaffEdit | null>(null);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(async () => {
    try {
      setPeople(await call('list_staff'));
      setRoles(await call('list_roles'));
    } catch (cause) {
      report(cause);
    }
  }, [report]);

  useEffect(() => {
    void load();
    // The one staff list: what the owner changed on the phone comes down when this screen
    // opens, and the list is read again once it has. Quiet when there is no cloud.
    call('pull_from_cloud')
      .then(() => load())
      .catch(() => undefined);
  }, [load]);

  /** Everything about them comes from Rust, so the dialog starts from what is really there. */
  const edit = (id: string) => {
    call('staff_details', { staffId: id })
      .then((person) => {
        setViewing(null);
        setEditing(editOf(person));
      })
      .catch(report);
  };

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
          {STATUS_WORDS[p.status] ?? p.status}
        </Badge>
      ),
    },
    {
      key: 'do',
      header: '',
      render: (p) => (
        // The row opens the card; the button is its own press.
        <div className="mb-row mb-row--end" onClick={(event) => event.stopPropagation()}>
          <Button size="sm" onClick={() => edit(p.id)}>
            Edit
          </Button>
        </div>
      ),
    },
  ];

  return (
    <>
      <div className="mb-row mb-row--end">
        <Button variant="primary" onClick={() => setEditing(blankPerson(freshId('staff')))}>
          Add somebody
        </Button>
      </div>

      <Table
        rows={people}
        columns={columns}
        rowKey={(p) => p.id}
        onRow={(p) => setViewing(p.id)}
      />

      {viewing ? (
        <PersonCard
          staffId={viewing}
          onClose={() => setViewing(null)}
          onEdit={() => edit(viewing)}
          onFailed={report}
        />
      ) : null}

      {editing ? (
        <EditPerson
          person={editing}
          isNew={!people.some((p) => p.id === editing.id)}
          roles={roles}
          onClose={() => setEditing(null)}
          onSaved={(saved) => {
            setPeople(saved);
            setEditing(null);
            toast.show('ok', 'Saved.');
          }}
        />
      ) : null}
    </>
  );
}

/** Everything about one person, to read. */
function PersonCard({
  staffId,
  onClose,
  onEdit,
  onFailed,
}: {
  staffId: string;
  onClose: () => void;
  onEdit: () => void;
  onFailed: (cause: unknown) => void;
}) {
  const [person, setPerson] = useState<StaffDetailView | null>(null);

  useEffect(() => {
    call('staff_details', { staffId }).then(setPerson).catch(onFailed);
  }, [staffId, onFailed]);

  if (!person) return null;

  const emergency =
    person.emergencyName === '' && person.emergencyPhone === ''
      ? '—'
      : [person.emergencyName, person.emergencyPhone].filter((w) => w !== '').join(' · ');

  return (
    <Modal
      open
      title={person.name}
      onClose={onClose}
      wide
      actions={
        <>
          <Button variant="quiet" onClick={onClose}>
            Close
          </Button>
          <Button variant="primary" onClick={onEdit}>
            Edit
          </Button>
        </>
      }
    >
      <Facts>
        <Fact label="Role">{person.role ?? 'No role yet'}</Fact>
        <Fact label="Status">
          {STATUS_WORDS[person.status] ?? person.status}
          {person.leftOn !== '' ? ` on ${person.leftOn}` : ''}
        </Fact>
        <Fact label="PIN">{person.hasPin ? 'Set' : 'None'}</Fact>
        <Fact label="Mobile">{person.phone === '' ? '—' : `+91 ${person.phone}`}</Fact>
        <Fact label="What they do">{person.designation || '—'}</Fact>
        <Fact label="Which part of the shop">{person.department || '—'}</Fact>
        <Fact label="Working">{WORKING_WORDS[person.employmentType] ?? person.employmentType}</Fact>
        <Fact label="Joined">{person.joined || '—'}</Fact>
        <Fact label="Where they live">{person.address || '—'}</Fact>
        <Fact label="In an emergency">{emergency}</Fact>
        <Fact label="ID they gave you">{person.idProof || '—'}</Fact>
        {person.salarySays !== '' ? <Fact label="Salary">{person.salarySays}</Fact> : null}
      </Facts>
    </Modal>
  );
}

/** One dialog for the whole person: who they are at the sign-in screen, and what they do. */
function EditPerson({
  person,
  isNew,
  roles,
  onClose,
  onSaved,
}: {
  person: StaffEdit;
  isNew: boolean;
  roles: readonly RoleView[];
  onClose: () => void;
  onSaved: (people: readonly PersonView[]) => void;
}) {
  const [edit, setEdit] = useState(person);
  const [pinAgain, setPinAgain] = useState('');
  const [problem, setProblem] = useState('');
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const set = <K extends keyof StaffEdit>(key: K, value: StaffEdit[K]) =>
    setEdit((e) => ({ ...e, [key]: value }));
  const digits = (typed: string) => typed.replace(/[^0-9]/g, '');

  const save = () => {
    // The same rules Rust holds — `mb_auth::pin::PIN_DIGITS`, and a new person needs one.
    if (edit.pin !== '' || isNew) {
      if (edit.pin.length !== PIN_DIGITS) {
        setProblem(`A PIN is ${PIN_DIGITS} digits.`);
        return;
      }
      if (edit.pin !== pinAgain) {
        setProblem('The two PINs are not the same. Type it again.');
        return;
      }
    }
    setProblem('');
    setBusy(true);
    call('save_staff_member', { staff: edit })
      .then(onSaved)
      .catch((cause: unknown) => {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      })
      .finally(() => setBusy(false));
  };

  return (
    <Modal
      open
      title={isNew ? 'Add somebody' : person.name}
      onClose={onClose}
      wide
      actions={
        <>
          <Button variant="quiet" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={busy} onClick={save}>
            Save
          </Button>
        </>
      }
    >
      <Stack gap="group">
        <Fields columns>
          <Input
            label="Name"
            value={edit.name}
            autoFocus
            onChange={(e) => set('name', e.target.value)}
          />
          <Select
            label="Role"
            value={edit.roleId ?? ''}
            onChange={(e) => set('roleId', e.target.value === '' ? null : e.target.value)}
            options={[
              { value: '', label: 'Choose a role' },
              ...roles.map((r) => ({ value: r.id, label: r.name })),
            ]}
          />
          <Input
            label={isNew ? 'PIN' : 'New PIN'}
            hint={
              isNew
                ? `${PIN_DIGITS} digits. It is stored scrambled and cannot be read back.`
                : 'Leave it empty to keep the one they have.'
            }
            maxLength={PIN_DIGITS}
            value={edit.pin}
            type="password"
            inputMode="numeric"
            onChange={(e) => set('pin', digits(e.target.value))}
          />
          <Input
            label="The same PIN again"
            maxLength={PIN_DIGITS}
            value={pinAgain}
            type="password"
            inputMode="numeric"
            onChange={(e) => setPinAgain(digits(e.target.value))}
          />
          <PhoneInput
            label="Mobile"
            value={edit.phone}
            onChange={(phone) => set('phone', phone)}
          />
          {/* A new person works here; only somebody already on the list can be anything else. */}
          {isNew ? null : (
            <Select
              label="Status"
              value={edit.status}
              onChange={(e) => {
                set('status', e.target.value);
                if (e.target.value !== 'left') set('leftOn', '');
              }}
              options={[
                { value: 'active', label: 'Works here' },
                { value: 'suspended', label: 'Suspended — cannot sign in' },
                { value: 'left', label: 'Has left' },
              ]}
            />
          )}
          {edit.status === 'left' ? (
            <Input
              label="The day they left"
              hint="It takes them off the payroll and off the roster — nothing about their past is deleted."
              value={edit.leftOn}
              placeholder="2026-08-31"
              onChange={(e) => set('leftOn', e.target.value)}
            />
          ) : null}
        </Fields>
        {problem ? (
          <p className="mb-lock__problem" role="alert">
            {problem}
          </p>
        ) : null}

        <SectionHeader title="At work" />
        <Fields columns>
          <Input
            label="What they do"
            hint="Cook, cashier, cleaner. It goes on the payslip."
            value={edit.designation}
            onChange={(e) => set('designation', e.target.value)}
          />
          <Input
            label="Which part of the shop"
            hint="Kitchen, counter, delivery. Leave it blank if you have only one."
            value={edit.department}
            onChange={(e) => set('department', e.target.value)}
          />
          <Select
            label="Working"
            value={edit.employmentType}
            onChange={(e) => set('employmentType', e.target.value)}
            options={Object.entries(WORKING_WORDS).map(([value, label]) => ({ value, label }))}
          />
          <Input
            label="Where they live"
            value={edit.address}
            onChange={(e) => set('address', e.target.value)}
          />
          <Input
            label="Who to call in an emergency"
            value={edit.emergencyName}
            onChange={(e) => set('emergencyName', e.target.value)}
          />
          <PhoneInput
            label="On this number"
            value={edit.emergencyPhone}
            onChange={(phone) => set('emergencyPhone', phone)}
          />
          <Input
            label="ID they gave you"
            hint="Aadhaar number, licence number — whatever you keep on file."
            value={edit.idProof}
            onChange={(e) => set('idProof', e.target.value)}
          />
        </Fields>
      </Stack>
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
