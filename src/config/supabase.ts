// Supabase backend endpoints for the POS.
// The POS never talks to Postgres directly — licensing (and later bill sync)
// goes through Edge Functions in the MB-backend repo.
//
// The anon key is a public, RLS-restricted key and is safe to ship in the app.
// The service_role key must NEVER appear in this codebase.

export const SUPABASE_FUNCTIONS_URL =
  "https://rlvygwituwywofwcwjsf.supabase.co/functions/v1";

export const SUPABASE_ANON_KEY = import.meta.env.VITE_SUPABASE_ANON_KEY as string;
