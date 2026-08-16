/**
 * Where the screens start.
 *
 * Three providers and a shell, in an order that matters: the theme goes on
 * before anything paints (budget S3 — *"something on screen, not white"*), and
 * the toast system wraps the shell so the shell itself can report a failure.
 */

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import './theme/tokens.css';
import { ThemeProvider } from './theme/ThemeProvider';
import { ToastProvider } from './kit';
import { Shell } from './shell/Shell';
import { Display } from './display/Display';

/**
 * **P29, scope 7.8 — the same app, facing the other way.**
 *
 * The customer display is a second window on the same bundle, told apart by
 * one query parameter. Not a second program: a second program would need its
 * own copy of the theme, its own idea of what a bill line looks like, and its
 * own bug when the two disagree about a total.
 *
 * It gets no `ToastProvider` and no `Shell` — a customer has nothing to be
 * notified about and nothing to navigate. That is also why it cannot take the
 * keyboard: there is nothing on the page to take it with.
 */
const facingTheCustomer =
  typeof window !== 'undefined' &&
  new URLSearchParams(window.location.search).get('display') === '1';

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <ThemeProvider>
        {facingTheCustomer ? (
          <Display />
        ) : (
          <ToastProvider>
            <Shell />
          </ToastProvider>
        )}
      </ThemeProvider>
    </StrictMode>,
  );
}
