// A FIXTURE THAT MUST BE REJECTED by scripts/check-no-money.mjs (P08 T4).
//
// This is R8's first violation, and it is always three characters long.
export function wrong(subtotal: number, tax: number): number {
  const total = subtotal + tax;
  return total;
}
