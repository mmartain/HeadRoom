import { invoke } from "@tauri-apps/api/core";
import { ProviderRow } from "./ProviderRow";
import { getPlugin, listEnabledPlugins } from "../providers/registry";
import type { UsageSnapshot } from "../providers/types";
import { beginWindowDrag } from "../lib/windowDrag";

type Props = {
  snapshots: UsageSnapshot[];
  enabled: Record<string, boolean>;
  lastUpdated: Date | null;
  loading: boolean;
  onRefresh: () => void;
  onOpenSettings: () => void;
  overlayVisible: boolean;
  onToggleOverlay: () => void;
  updateAvailable: { version: string } | null;
  onInstallUpdate: () => void;
};

export function Flyout({
  snapshots,
  enabled,
  lastUpdated,
  loading,
  onRefresh,
  onOpenSettings,
  overlayVisible,
  onToggleOverlay,
  updateAvailable,
  onInstallUpdate,
}: Props) {
  const plugins = listEnabledPlugins(enabled);
  const ordered = plugins
    .map((p) => snapshots.find((s) => s.providerId === p.id))
    .filter((s): s is UsageSnapshot => Boolean(s));

  const updatedLabel = lastUpdated
    ? `refreshed ${formatRelative(lastUpdated)}`
    : loading
      ? "loading…"
      : "not yet refreshed";

  async function minimizeFlyout() {
    await invoke("hide_flyout");
  }

  return (
    <div className="flyout">
      <header className="flyout-header drag-surface" onMouseDown={beginWindowDrag}>
        <div>
          <p className="brand">HeadRoom</p>
          <p className="sub">{updatedLabel}</p>
        </div>
        <button
          type="button"
          className="status-minimize"
          data-no-drag
          title="Minimize"
          aria-label="Minimize"
          onMouseDown={(e) => e.stopPropagation()}
          onClick={() => void minimizeFlyout()}
        >
          <span aria-hidden>─</span>
        </button>
      </header>

      {updateAvailable && (
        <div className="update-banner">
          <span>HeadRoom v{updateAvailable.version} available</span>
          <button type="button" onClick={onInstallUpdate}>
            Install
          </button>
        </div>
      )}

      <div className="flyout-body">
        {ordered.length === 0 && !loading && (
          <p className="empty">Enable a provider in Settings to start tracking.</p>
        )}
        {ordered.map((snap) => (
          <ProviderRow
            key={snap.providerId}
            snapshot={snap}
            accentColor={getPlugin(snap.providerId)?.accentColor ?? "#9ca3af"}
          />
        ))}
      </div>

      <footer className="flyout-footer">
        <button type="button" onClick={onRefresh} disabled={loading}>
          {loading ? "Refreshing…" : "Refresh"}
        </button>
        <button type="button" onClick={onToggleOverlay}>
          {overlayVisible ? "Hide top bar" : "Top status bar"}
        </button>
        <button type="button" onClick={onOpenSettings}>
          Settings
        </button>
      </footer>
    </div>
  );
}

function formatRelative(d: Date): string {
  const sec = Math.max(0, Math.round((Date.now() - d.getTime()) / 1000));
  if (sec < 10) return "just now";
  if (sec < 60) return `${sec}s ago`;
  const min = Math.round(sec / 60);
  if (min < 60) return `${min}m ago`;
  return d.toLocaleTimeString();
}
