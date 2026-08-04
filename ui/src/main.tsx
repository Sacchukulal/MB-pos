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

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <ThemeProvider>
        <ToastProvider>
          <Shell />
        </ToastProvider>
      </ThemeProvider>
    </StrictMode>,
  );
}
