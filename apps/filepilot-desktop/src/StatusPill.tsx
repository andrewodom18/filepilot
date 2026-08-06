import type { TaskStatus } from "./types";

export function StatusPill({ status }: { status: TaskStatus }) {
  return <span className={`status-pill ${status}`} role="status">{status}</span>;
}
