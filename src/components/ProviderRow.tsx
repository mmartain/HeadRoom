import type { UsageSnapshot, UsageWindow } from "../providers/types";
import { windowBarColor } from "../lib/windowColors";

function formatReset(resetsAt: string | null): string | null {
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

/** Keep the flyout compact: at most three usage windows per provider. */
function pickWindows(windows: UsageWindow[]): UsageWindow[] {
  const withPct = windows.filter((w) => w.usedPercent != null);
  const without = windows.filter((w) => w.usedPercent == null);
  const picked = [...withPct.slice(0, 3)];
  if (picked.length < 3 && without[0]) picked.push(without[0]);
  return picked.slice(0, 3);
}

function WindowBar({
  window,
  color,
}: {
  window: UsageWindow;
  color: string;
}) {
  const used = window.usedPercent;
  return (
    <div className="window-row">
      <div className="window-meta">
        <span className="window-label">
          <span className="window-swatch" style={{ background: color }} />
          {window.label}
        </span>
        <span className="window-remaining">
          {window.remainingLabel ??
            (used != null ? `${Math.max(0, 100 - used).toFixed(0)}% left` : "—")}
        </span>
      </div>
      {used != null && (
        <div className="bar-track" aria-hidden>
          <div
            className="bar-fill"
            style={{
              width: `${Math.min(100, Math.max(0, used))}%`,
              background: color,
            }}
          />
        </div>
      )}
    </div>
  );
}

type Props = {
  snapshot: UsageSnapshot;
  accentColor: string;
};

export function ProviderRow({ snapshot, accentColor }: Props) {
  const windows = pickWindows(snapshot.windows);
  const reset =
    formatReset(windows.find((w) => w.resetsAt)?.resetsAt ?? null) ??
    formatReset(snapshot.windows.find((w) => w.resetsAt)?.resetsAt ?? null);

  return (
    <article className="provider-row" style={{ ["--accent" as string]: accentColor }}>
      <header className="provider-header">
        <span className="accent-tick" />
        <h2>{snapshot.displayName}</h2>
        {reset && <span className="provider-reset">{reset}</span>}
        <span className={`status status-${snapshot.status}`}>{statusLabel(snapshot.status)}</span>
      </header>

      {snapshot.status === "ok" && windows.length > 0 && (
        <div className="windows">
          {windows.map((w, i) => (
            <WindowBar
              key={w.id}
              window={w}
              color={windowBarColor(i, accentColor)}
            />
          ))}
        </div>
      )}

      {snapshot.status !== "ok" && snapshot.errorMessage && (
        <p className="provider-message">{snapshot.errorMessage}</p>
      )}

      {snapshot.status === "ok" && windows.length === 0 && (
        <p className="provider-message">No usage windows reported.</p>
      )}
    </article>
  );
}

function statusLabel(status: UsageSnapshot["status"]): string {
  switch (status) {
    case "ok":
      return "OK";
    case "needs_auth":
      return "Connect";
    case "error":
      return "Error";
    case "disabled":
      return "Off";
    default:
      return status;
  }
}
