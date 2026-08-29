/** The tills this shop has. */

import { useCallback, useEffect, useState } from 'react';

import { Badge, Button, Card, Input, Modal, SectionHeader, Spinner, useToast } from '../kit';
import { call, inApp, isUiError, subscribe } from '../ipc/call';
import type { TerminalView } from '../ipc/generated/TerminalView';
import type { TillsView } from '../ipc/generated/TillsView';

/** What a person is typing into the join box. */
type Joining = {
  address: string;
  fingerprint: string;
  token: string;
  name: string;
  prefix: string;
};

const NOTHING_TYPED: Joining = {
  address: '',
  fingerprint: '',
  token: '',
  name: '',
  prefix: '',
};

export function Tills() {
  const [view, setView] = useState<TillsView | null>(null);
  const [editing, setEditing] = useState<TerminalView | null>(null);
  const [name, setName] = useState('');
  const [prefix, setPrefix] = useState('');
  const [joining, setJoining] = useState<Joining | null>(null);
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  useEffect(() => {
    if (!inApp()) return;
    call('tills').then(setView).catch(complain);
  }, [complain]);

  // Rust pushes; React subscribes.
  useEffect(() => {
    if (!inApp()) return undefined;
    let stop: (() => void) | undefined;
    subscribe((message) => {
      if (message.kind === 'tills') {
        call('tills')
          .then(setView)
          .catch(() => undefined);
      }
    })
      .then((off) => {
        stop = off;
      })
      .catch(() => undefined);
    return () => stop?.();
  }, []);

  const open = (till: TerminalView) => {
    setEditing(till);
    setName(till.name);
    setPrefix(till.prefix);
  };

  const save = () => {
    if (!editing) return;
    setBusy(true);
    call('save_till', { edit: { id: editing.id, name, prefix } })
      .then((next) => {
        setView(next);
        setEditing(null);
        toast.show('ok', 'Saved.');
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  const makeMain = (till: TerminalView) => {
    setBusy(true);
    call('make_master', { id: till.id })
      .then((next) => {
        setView(next);
        toast.show('ok', `${till.name} is now the main till.`);
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  const join = () => {
    if (!joining) return;
    setBusy(true);
    call('join_master', joining as never)
      .then((next) => {
        setView(next);
        setJoining(null);
        toast.show('ok', 'This till joined the shop. Restart it to start billing.');
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  const sendNow = () => {
    setBusy(true);
    call('send_waiting_bills')
      .then(setView)
      .catch(complain)
      .finally(() => setBusy(false));
  };

  if (!view) return <Spinner label="Looking at the tills" />;

  return (
    <div className="mb-tills">
      <SectionHeader
        title="Tills"
        note="Each till prints its own bill numbers, so two counters never wait for each other."
        action={<Badge tone={view.isMaster ? 'ok' : 'neutral'}>
          {view.isMaster ? 'This is the main till' : 'This is a second till'}
        </Badge>}
      />

      {view.awaySays ? (
        <Card className="mb-tills__away">
          <strong>{view.awaySays}</strong>
        </Card>
      ) : null}

      {/*
        What this till is holding, with the button that pushes it across — the same call the
        background sender makes, so pressing it can only be early, never different.
      */}
      {view.waitingSays ? (
        <Card className="mb-tills__waiting">
          <span>{view.waitingSays}</span>
          <Button size="sm" variant="quiet" disabled={busy} onClick={sendNow}>
            Send now
          </Button>
        </Card>
      ) : null}

      <Card>
        <p className="mb-tills__note">{view.limitSays}</p>
        {view.tills.map((till) => (
          <div className="mb-tills__till" key={till.id}>
            <div>
              <strong>{till.name}</strong>
              {till.isMaster ? <Badge tone="ok">Main till</Badge> : null}
              {till.isThisOne ? <Badge tone="neutral">This one</Badge> : null}
              {/* The whole sentence, written in Rust. */}
              <p className="mb-tills__note">
                {till.numbersSay} Last seen {till.lastSeen}.
              </p>
            </div>
            <div className="mb-row--end">
              {view.mayManage && !till.isMaster ? (
                <Button size="sm" variant="quiet" disabled={busy} onClick={() => makeMain(till)}>
                  Make it the main till
                </Button>
              ) : null}
              {view.mayManage ? (
                <Button size="sm" variant="quiet" onClick={() => open(till)}>
                  Change
                </Button>
              ) : null}
            </div>
          </div>
        ))}
      </Card>

      {view.mayManage ? (
        <Card>
          <SectionHeader
            title="Add this computer to a shop"
            note={
              <>
                Do this on the NEW till, not on the main one. On the main till
                open Settings, Phones, and press &ldquo;Add a phone&rdquo; — it
                shows the address, the security code and a short code to type
                here. Somebody at the main till has to press Allow.
              </>
            }
          />
          <div className="mb-row--end">
            <Button variant="primary" onClick={() => setJoining(NOTHING_TYPED)}>
              Join a shop
            </Button>
          </div>
        </Card>
      ) : null}

      <Modal
        open={editing !== null}
        title={editing ? `Change ${editing.name}` : 'Change this till'}
        onClose={() => setEditing(null)}
        actions={
          <>
            <Button variant="quiet" onClick={() => setEditing(null)}>
              Cancel
            </Button>
            <Button variant="primary" disabled={busy} onClick={save}>
              Save
            </Button>
          </>
        }
      >
        <Input
          label="What this till is called"
          value={name}
          onChange={(e) => setName(e.currentTarget.value)}
        />
        <Input
          label="What goes in front of its bill numbers"
          value={prefix}
          hint="A/ or B/ — so this till's bills read A/0001 and the other's read B/0001. No two tills may use the same one."
          onChange={(e) => setPrefix(e.currentTarget.value)}
        />
      </Modal>

      <Modal
        open={joining !== null}
        title="Join a shop"
        onClose={() => setJoining(null)}
        actions={
          <>
            <Button variant="quiet" onClick={() => setJoining(null)}>
              Cancel
            </Button>
            <Button variant="primary" disabled={busy} onClick={join}>
              Join
            </Button>
          </>
        }
      >
        <Input
          label="The main till's address"
          value={joining?.address ?? ''}
          hint="Shown on the main till, like https://192.168.0.104:7331"
          onChange={(e) =>
            setJoining((was) => (was ? { ...was, address: e.currentTarget.value } : was))
          }
        />
        <Input
          label="The main till's security code"
          value={joining?.fingerprint ?? ''}
          hint="Also on the main till's Phones screen. This is what stops a stranger on the WiFi pretending to be it."
          onChange={(e) =>
            setJoining((was) => (was ? { ...was, fingerprint: e.currentTarget.value } : was))
          }
        />
        <Input
          label="The short code"
          value={joining?.token ?? ''}
          hint="It stops working after a few minutes."
          onChange={(e) =>
            setJoining((was) => (was ? { ...was, token: e.currentTarget.value } : was))
          }
        />
        <Input
          label="What to call this till"
          value={joining?.name ?? ''}
          hint="Somebody at the main till reads this before pressing Allow."
          onChange={(e) =>
            setJoining((was) => (was ? { ...was, name: e.currentTarget.value } : was))
          }
        />
        <Input
          label="What goes in front of its bill numbers"
          value={joining?.prefix ?? ''}
          hint="B/ if the main till uses A/. Without it two tills would print the same bill number."
          onChange={(e) =>
            setJoining((was) => (was ? { ...was, prefix: e.currentTarget.value } : was))
          }
        />
      </Modal>
    </div>
  );
}
