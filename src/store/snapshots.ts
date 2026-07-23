import { invoke } from "@tauri-apps/api/core";
import { defaultEnabledMap, listPlugins } from "../providers/registry";
import type { UsageSnapshot } from "../providers/types";

export type AppSettings = {
  enabled: Record<string, boolean>;
  overlayVisible: boolean;
  /** 0 = fully transparent background, 100 = solid. */
  overlayOpacity: number;
  /** Hide the top bar when the mouse approaches so clicks pass through. */
  overlayHideNearMouse: boolean;
  alertThresholds: number[];
  pollIntervalSec: number;
};

export function mergeSettings(raw: Record<string, unknown> | null | undefined): AppSettings {
  const defaults = defaultEnabledMap();
  const enabled = {
    ...defaults,
    ...((raw?.enabled as Record<string, boolean> | undefined) ?? {}),
  };
  const opacityRaw = raw?.overlayOpacity;
  const overlayOpacity =
    typeof opacityRaw === "number" && Number.isFinite(opacityRaw)
      ? Math.min(100, Math.max(0, opacityRaw))
      : 92;
  return {
    enabled,
    overlayVisible: Boolean(raw?.overlayVisible ?? false),
    overlayOpacity,
    overlayHideNearMouse: Boolean(raw?.overlayHideNearMouse ?? true),
    alertThresholds: Array.isArray(raw?.alertThresholds)
      ? (raw!.alertThresholds as number[])
      : [80, 95],
    pollIntervalSec:
      typeof raw?.pollIntervalSec === "number" && raw.pollIntervalSec >= 30
        ? raw.pollIntervalSec
        : 120,
  };
}

export async function loadSettings(): Promise<AppSettings> {
  const raw = await invoke<Record<string, unknown>>("get_settings");
  return mergeSettings(raw);
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  await invoke("set_settings", { settings });
}

export async function fetchSnapshots(enabled: Record<string, boolean>): Promise<UsageSnapshot[]> {
  const ids = listPlugins()
    .filter((p) => enabled[p.id] === true)
    .map((p) => p.id);

  if (ids.length === 0) return [];

  const results = await invoke<UsageSnapshot[]>("fetch_all_usage", { providerIds: ids });

  // Fill disabled placeholders for registry order consistency in UI when needed
  return results;
}

export function worstRemainingPercent(snapshots: UsageSnapshot[]): number | null {
  let worst: number | null = null;
  for (const snap of snapshots) {
    if (snap.status !== "ok") continue;
    for (const w of snap.windows) {
      if (w.usedPercent == null) continue;
      const remaining = 100 - w.usedPercent;
      if (worst == null || remaining < worst) worst = remaining;
    }
  }
  return worst;
}

export type AlertKey = string;

/** Tracks which threshold toasts already fired for provider+window this cycle. */
export class AlertTracker {
  private fired = new Set<AlertKey>();

  key(providerId: string, windowId: string, threshold: number): AlertKey {
    return `${providerId}:${windowId}:${threshold}`;
  }

  /** Returns newly crossed thresholds for this snapshot set. */
  evaluate(
    snapshots: UsageSnapshot[],
    thresholds: number[],
  ): Array<{ providerId: string; displayName: string; windowLabel: string; usedPercent: number; threshold: number }> {
    const hits: Array<{
      providerId: string;
      displayName: string;
      windowLabel: string;
      usedPercent: number;
      threshold: number;
    }> = [];

    const sorted = [...thresholds].sort((a, b) => a - b);

    for (const snap of snapshots) {
      if (snap.status !== "ok") continue;
      for (const w of snap.windows) {
        if (w.usedPercent == null) continue;
        for (const t of sorted) {
          const k = this.key(snap.providerId, w.id, t);
          if (w.usedPercent >= t) {
            if (!this.fired.has(k)) {
              this.fired.add(k);
              hits.push({
                providerId: snap.providerId,
                displayName: snap.displayName,
                windowLabel: w.label,
                usedPercent: w.usedPercent,
                threshold: t,
              });
            }
          } else {
            // Reset when usage drops below threshold (new billing window)
            this.fired.delete(k);
          }
        }
      }
    }
    return hits;
  }
}
