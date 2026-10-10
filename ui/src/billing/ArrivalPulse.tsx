/** Replace only this passive surface when an arrival repeats; keep focus and controls intact. */
export function ArrivalPulse({ generation }: { generation?: number }) {
  return generation === undefined ? null : (
    <span key={generation} className="mb-arrival-pulse" aria-hidden="true" />
  );
}
