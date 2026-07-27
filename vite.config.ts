import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

/**
 * v1.3.0 shipped an installer with no Supabase anon key: `.env` is gitignored
 * and the release workflow never passed the value, so `createClient()` threw
 * "supabaseKey is required." and the whole mobile-orders bridge died while
 * licensing (plain fetch) kept working. A keyless installer must never be
 * publishable again, so a production build refuses to start without it.
 */
function requireAnonKey(mode: string, root: string): void {
  // "" = load every VITE_* var from .env files as well as the environment,
  // so this works both for `npm run build` and for CI's bare environment.
  const env = loadEnv(mode, root, "");
  const key = (env.VITE_SUPABASE_ANON_KEY ?? "").trim();
  if (key.length >= 20) return;

  throw new Error(
    [
      "",
      "  BUILD ABORTED — VITE_SUPABASE_ANON_KEY is missing or too short.",
      "",
      `  Got: ${key.length === 0 ? "empty/undefined" : `${key.length} characters`}`,
      "",
      "  Without it the app ships an undefined Supabase key, realtime dies on",
      "  startup, and mobile ordering is silently dead in the installer.",
      "",
      "  Set it in ONE of these places:",
      "    - local builds : MB-pos/.env  (gitignored, never commit it)",
      "    - CI releases  : GitHub -> Sacchukulal/MB-pos -> Settings ->",
      "                     Secrets and variables -> Actions ->",
      "                     VITE_SUPABASE_ANON_KEY,",
      "                     and pass it in the env: block of the tauri-action",
      "                     step in .github/workflows/release.yml",
      "",
    ].join("\n"),
  );
}

// https://vite.dev/config/
export default defineConfig(async ({ command, mode }) => {
  // @ts-expect-error process is a nodejs global
  if (command === "build") requireAnonKey(mode, process.cwd());

  return {
    plugins: [react()],

    // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
    //
    // 1. prevent Vite from obscuring rust errors
    clearScreen: false,
    // 2. tauri expects a fixed port, fail if that port is not available
    server: {
      port: 1420,
      strictPort: true,
      host: host || false,
      hmr: host
        ? {
            protocol: "ws",
            host,
            port: 1421,
          }
        : undefined,
      watch: {
        // 3. tell Vite to ignore watching `src-tauri`
        ignored: ["**/src-tauri/**"],
      },
    },
  };
});
