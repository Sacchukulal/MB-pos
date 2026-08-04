import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

// The counter is a desktop app, not a website: it is served from disk by
// Tauri, it has no CDN and no cloud (R10), and every byte counts against S4's
// 20 MB installer.
export default defineConfig({
  plugins: [react()],
  // Tauri serves the built files from a file:// style origin, so assets must
  // be referenced relatively rather than from the root.
  base: './',
  build: {
    outDir: 'dist',
    // WebView2 on Windows 10/11 is current Chromium, so there is nothing to
    // transpile down to. Targeting old browsers would cost bundle size for
    // machines that cannot run this app anyway.
    target: 'chrome120',
    sourcemap: true,
  },
  server: {
    port: 5173,
    strictPort: true,
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./tests/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}', 'tests/**/*.test.{ts,tsx}'],
  },
});
