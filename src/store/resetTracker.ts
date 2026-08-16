import type { UsageSnapshot } from "../providers/types";

export type ResetHit = {
  providerId: string;
  displayName: string;
  windowLabel: string;
  usedPercent: number | null;
};

/**
 * Minimum gap (ms) between two `resetsAt` timestamps that counts as a
 * rollover. Guards against provider timestamp jitter and hypothetical
 * rolling/relative timestamps; every real window is >= 5h, so a genuine
 * reset always exceeds this.
 */
const MIN_RESET_DELTA_MS = 60 * 60 * 1000; // 1 hour

type LastSeen = { provider: string; resetsAt: number | null };

function parseDate(value: string | null): number | null {
  if (!value) return null;
  const t = Date.parse(value);
  return Number.isNaN(t) ? null : t;
}

/**
 * Persistent reset detection. Storage key format: `<providerId>:<windowId>`
 * (same convention as AlertTracker).
 *
 * - `seed()` loads last-seen timestamps from the previous session so a reset
 *   that happened while the app was closed fires on the first poll.
 * - `evaluate()` fires once per window per cycle when `resetsAt` moves to a
 *   later instant (>= MIN_RESET_DELTA_MS later).
 * - `snapshot()` serializes the fresh state for storage.
 */
export class ResetTracker {
  private last = new Map<string, LastSeen>();

  seed(record: Record<string, string | null>): void {
    this.last.clear();
    for (const [key, raw] of Object.entries(record)) {
      const sep = key.indexOf(":");
      if (sep <= 0) continue; // malformed key
      this.last.set(key, { provider: key.slice(0, sep), resetsAt: parseDate(raw) });
    }
  }

  /**
   * Serialize for storage; only windows of enabled providers are kept so a
   * long-disabled provider re-seeds fresh instead of firing stale-baseline hits.
   */
  snapshot(enabled: Record<string, boolean>): Record<string, string | null> {
    const out: Record<string, string | null> = {};
    for (const [key, seen] of this.last) {
      if (enabled[seen.provider] === true) {
        out[key] = seen.resetsAt != null ? new Date(seen.resetsAt).toISOString() : null;
      }
    }
    return out;
  }

  evaluate(snapshots: UsageSnapshot[]): ResetHit[] {
    const hits: ResetHit[] = [];
    for (const snap of snapshots) {
      if (snap.status !== "ok") continue;
      for (const w of snap.windows) {
        const key = `${snap.providerId}:${w.id}`;
        const next = parseDate(w.resetsAt);
        const prev = this.last.get(key);
        this.last.set(key, { provider: snap.providerId, resetsAt: next });
        if (prev == null || prev.resetsAt == null || next == null) continue;
        if (next - prev.resetsAt >= MIN_RESET_DELTA_MS) {
          hits.push({
            providerId: snap.providerId,
            displayName: snap.displayName,
            windowLabel: w.label,
            usedPercent: w.usedPercent,
          });
        }
      }
    }
    return hits;
  }
}
