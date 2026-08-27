/** The shell, and the one thing it must not do. */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

// The title bar draws its own minimise/maximise/close, so it asks Tauri for the window.
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    minimize: () => undefined,
    toggleMaximize: () => undefined,
    close: () => undefined,
  }),
}));

const call = vi.fn();
const listeners: Record<string, (payload: unknown) => void> = {};
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: () => false,
  isLicenceRefusal: () => false,
  subscribe: (name: string, fn: (payload: unknown) => void) => {
    listeners[name] = fn;
    return Promise.resolve(() => undefined);
  },
}));

/** How many people are signed in, from the shell's point of view. */
let signedInAs: string | null = null;

const { Shell } = await import('../src/shell/Shell');
const { ToastProvider } = await import('../src/kit');
const { ThemeProvider } = await import('../src/theme/ThemeProvider');

/** Every command the shell and the billing screen ask for on the way up. */
function answer(command: string): Promise<unknown> {
  switch (command) {
    case 'lock_state':
      return Promise.resolve({
        signedInAs,
        role: signedInAs === null ? null : 'Owner',
        people: [
          {
            id: 'staff_1',
            name: 'Meena',
            code: null,
            role: 'Owner',
            status: 'active',
            hasPin: true,
            lockedOut: null,
            permissions: [],
            maxDiscountBp: null,
            maxDiscount: null,
          },
        ],
        canRecover: true,
      });
    case 'first_run':
      return Promise.resolve({
        needed: false,
        hasShop: true,
        hasDetails: true,
        hasPin: true,
        shopPath: 'C:/shop/magicbill.db',
        found: [],
        defaultFolder: 'C:/shop',
      });
    case 'app_status':
      return Promise.resolve({
        shopPath: 'C:/shop/magicbill.db',
        licence: '',
        licenceTone: 'ok',
        needsPin: false,
      });
    case 'menu_items':
      // Refused while locked, exactly as Rust does it.
      return signedInAs === null
        ? Promise.reject({ code: 'auth.locked', message: 'The screen is locked.' })
        : Promise.resolve([
            {
              id: 'itm_1',
              name: 'Masala Dosa',
              price: { paise: 8000n, text: '80.00' },
              rateLabel: '5%',
              categoryId: null,
              isOpenPrice: false,
            },
          ]);
    // The menu's route onto the billing screen since the tile grid came off it.
    case 'search_items':
      return signedInAs === null
        ? Promise.reject({ code: 'auth.locked', message: 'The screen is locked.' })
        : answer('menu_items');
    case 'login':
      signedInAs = 'Meena';
      return answer('lock_state');
    // The rest of what the counter asks for on the way up.
    case 'open_orders':
    case 'print_queue':
      return Promise.resolve([]);
    case 'device_manager':
      return Promise.resolve({ devices: [] });
    case 'current_cart':
      return Promise.resolve(null);
    default:
      return Promise.resolve(null);
  }
}

beforeEach(() => {
  signedInAs = null;
  call.mockReset();
  call.mockImplementation((command: string) => answer(command));
});
afterEach(cleanup);

function show() {
  return render(
    <ThemeProvider>
      <ToastProvider>
        <Shell />
      </ToastProvider>
    </ThemeProvider>,
  );
}

it('does not mount the screen behind the lock', async () => {
  show();
  await screen.findByText('Who is at the counter?');
  // Not one command that needs a session has been sent.
  expect(call).not.toHaveBeenCalledWith('menu_items');
});

/* Given longer than the default five seconds, and not because it is slow. */
it('mounts the screen with a session behind it, so its data arrives', { timeout: 20_000 }, async () => {
  show();
  await screen.findByText('Who is at the counter?');

  // Somebody signs in for real: pick the person, tap six digits, press the button.
  fireEvent.click(await screen.findByRole('button', { name: /Meena/ }));
  for (const digit of ['4', '8', '2', '9', '1', '3']) {
    fireEvent.click(await screen.findByRole('button', { name: digit }));
  }
  fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));

  // The screen mounts NOW, and this is the whole claim: its first command goes out with a
  // session behind it, so it comes back with the shop's menu in it rather than a refusal nobody
  // can see.
  await waitFor(() => expect(call).toHaveBeenCalledWith('menu_items'));

  /* And the shop's menu is really on the screen, not merely asked for. */
  fireEvent.change(screen.getByRole('searchbox', { name: /Item or table number/ }), {
    target: { value: 'dosa' },
  });
  expect(await screen.findByRole('option', { name: /Masala Dosa/ })).toBeTruthy();
});
