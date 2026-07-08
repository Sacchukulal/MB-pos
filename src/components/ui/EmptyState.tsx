import type { ReactNode } from "react";

interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  message?: string;
}

export default function EmptyState({ icon, title, message }: EmptyStateProps) {
  return (
    <div className="empty-block">
      {icon && <div className="empty-icon">{icon}</div>}
      <h3>{title}</h3>
      {message && <p>{message}</p>}
    </div>
  );
}
