interface StatCardProps {
  label: string;
  value: string;
  sub?: string;
}

/** Compact stat tile used across Reports and detail views. */
export default function StatCard({ label, value, sub }: StatCardProps) {
  return (
    <div className="stat-card">
      <span className="stat-card-label">{label}</span>
      <span className="stat-card-value">{value}</span>
      {sub && <span className="stat-card-sub">{sub}</span>}
    </div>
  );
}
