import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { afterEach, expect, it } from 'vitest';
import { Button, Input, Modal } from '../src/kit';

afterEach(cleanup);

it('focuses a persistently mounted dialog on each opening, wraps Tab and returns to its opener', async () => {
  function Example() {
    const [open, setOpen] = useState(false);
    return <><Button onClick={() => setOpen(true)}>Open</Button><Button>Outside</Button>
      <Modal open={open} title="Test" onClose={() => setOpen(false)} actions={<Button onClick={() => setOpen(false)}>Close</Button>}>
        <Input label="Name" /><Button disabled>Unavailable</Button>
      </Modal></>;
  }
  const user = userEvent.setup(); render(<Example />);
  const opener = screen.getByRole('button', { name: 'Open' });
  for (let attempt = 0; attempt < 2; attempt++) {
    await user.click(opener);
    expect(screen.getByLabelText('Name')).toHaveFocus();
    await user.tab({ shift: true });
    expect(screen.getByRole('button', { name: 'Close' })).toHaveFocus();
    await user.tab();
    expect(screen.getByLabelText('Name')).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(opener).toHaveFocus();
  }
});

it('keeps nested dialogs independent and restores each previous focus', async () => {
  function Example() {
    const [outer, setOuter] = useState(false); const [inner, setInner] = useState(false);
    return <><Button onClick={() => setOuter(true)}>Open outer</Button>
      <Modal open={outer} title="Outer" onClose={() => setOuter(false)}>
        <Button onClick={() => setInner(true)}>Open inner</Button>
        <Modal open={inner} title="Inner" onClose={() => setInner(false)}><Button>Only action</Button></Modal>
      </Modal></>;
  }
  const user = userEvent.setup(); render(<Example />);
  await user.click(screen.getByRole('button', { name: 'Open outer' }));
  const innerOpener = screen.getByRole('button', { name: 'Open inner' });
  await user.click(innerOpener);
  const action = within(screen.getByRole('dialog', { name: 'Inner' })).getByRole('button');
  await user.tab(); expect(action).toHaveFocus();
  await user.tab(); expect(action).toHaveFocus();
  await user.keyboard('{Escape}'); expect(innerOpener).toHaveFocus();
  await user.keyboard('{Escape}'); expect(screen.getByRole('button', { name: 'Open outer' })).toHaveFocus();
});
