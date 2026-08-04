// A FIXTURE THAT MUST BE REJECTED by scripts/check-tokens.mjs (P08 T4).
//
// A guard nobody has watched fail is a guard nobody knows is switched off.
// This file exists to be caught; it is never imported and never built.
export function Bad() {
  return <div style={{ color: '#ff0000', padding: '12px' }}>no</div>;
}
