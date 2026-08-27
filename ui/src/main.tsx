/** Where the screens start. */

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import './theme/tokens.css';
import { ThemeProvider } from './theme/ThemeProvider';
import { ToastProvider } from './kit';
import { Shell } from './shell/Shell';
import { Display } from './display/Display';

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
