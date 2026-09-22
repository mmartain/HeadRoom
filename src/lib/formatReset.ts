/** Human countdown from an ISO / parseable reset timestamp. */
export function formatReset(resetsAt: string | null | undefined): string | null {
  if (!resetsAt) return null;
  const d = new Date(resetsAt);
  if (Number.isNaN(d.getTime())) return resetsAt;
  const now = Date.now();
  const diffMs = d.getTime() - now;
  if (diffMs <= 0) return null;
  const hours = Math.floor(diffMs / 3_600_000);
  const mins = Math.floor((diffMs % 3_600_000) / 60_000);
  if (hours >= 48) {
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  }
  if (hours >= 1) return `${hours}h ${mins}m`;
  return `${mins}m`;
}
