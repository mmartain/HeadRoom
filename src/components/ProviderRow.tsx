import type { UsageSnapshot, UsageWindow } from "../providers/types";
import { windowBarColor } from "../lib/windowColors";
import { formatReset } from "../lib/formatReset";

/** Keep the flyout compact: up to 3 rate-limit windows + credits/budget always visible. */
function pickWindows(windows: UsageWindow[]): UsageWindow[] {
  const credits = windows.filter((w) => w.id === "credits");
  const withPct = windows.filter(
    (w) => w.usedPercent != null && w.id !== "credits",
  );
  const budgets = windows.filter(
    (w) =>
      w.id !== "credits" && w.usedPercent == null && w.remainingLabel != null,
  );
  const other = windows.filter(
    (w) =>
      w.id !== "credits" && w.usedPercent == null && w.remainingLabel == null,
  );
  const picked = [
    ...withPct.slice(0, 3),
    ...credits.slice(0, 1),
    ...budgets.slice(0, 2),
  ];
  if (picked.length === 0 && other[0]) picked.push(other[0]);
  return picked;
}

function WindowBar({
  window,
  color,
}: {
  window: UsageWindow;
  color: string;
}) {
  const used = window.usedPercent;
  const reset = formatReset(window.resetsAt);
  const remaining =
    window.remainingLabel ??
    (used != null ? `${Math.max(0, 100 - used).toFixed(0)}% left` : "—");
  return (
    <div className="window-row">
      <div className="window-meta">
        <span className="window-label">
          <span className="window-swatch" style={{ background: color }} />
          {window.label}
        </span>
        <span className="window-remaining">
          {remaining}
          {reset && <span className="window-reset"> · {reset}</span>}
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
