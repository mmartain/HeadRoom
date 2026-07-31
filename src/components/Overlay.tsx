import { listEnabledPlugins } from "../providers/registry";
import type { UsageSnapshot, UsageWindow } from "../providers/types";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { Menu } from "@tauri-apps/api/menu";
import type { MouseEvent as ReactMouseEvent } from "react";
import { beginWindowDrag } from "../lib/windowDrag";
import { windowBarColor } from "../lib/windowColors";

type Props = {
  snapshots: UsageSnapshot[];
  enabled: Record<string, boolean>;
  opacity: number;
};

const MAX_OVERLAY_BARS = 3;

/** Prefer windows that have a used% so bars always show useful fills. */
function meterWindows(snap: UsageSnapshot | undefined): UsageWindow[] {
  if (!snap || snap.status !== "ok") return [];
  return snap.windows
    .filter((w) => w.usedPercent != null)
    .slice(0, MAX_OVERLAY_BARS);
}

function meterTooltip(window: UsageWindow): string {
  const used = window.usedPercent;
  const detail =
    window.remainingLabel ??
    (used != null ? `${Math.max(0, 100 - used).toFixed(0)}% left` : "—");
  return `${window.label}: ${detail}`;
}

function primaryRemaining(snap: UsageSnapshot | undefined): {
  text: string;
  meters: UsageWindow[];
  title?: string;
} {
  if (!snap) return { text: "—", meters: [] };
  if (snap.status === "needs_auth") {
    return { text: "Connect", meters: [], title: snap.errorMessage };
  }
  if (snap.status === "error") {
    return { text: "Error", meters: [], title: snap.errorMessage };
  }
  if (snap.status === "disabled") return { text: "—", meters: [] };

  const meters = meterWindows(snap);
  const primary =
    meters[0] ??
    snap.windows.find((w) => w.usedPercent != null) ??
    snap.windows[0] ??
    null;

  if (!primary) return { text: "—", meters: [] };
  if (primary.usedPercent != null) {
    return {
      text: `${Math.max(0, 100 - primary.usedPercent).toFixed(0)}%`,
      meters,
    };
  }
  return {
    text: shorten(primary.remainingLabel ?? "—"),
    meters: [],
    title: primary.remainingLabel ?? undefined,
  };
}

function shorten(label: string): string {
  if (label.length <= 10) return label;
  return `${label.slice(0, 8)}…`;
}

export function Overlay({ snapshots, enabled, opacity }: Props) {
  const cells = listEnabledPlugins(enabled).map((plugin) => {
    const snap = snapshots.find((s) => s.providerId === plugin.id);
    const { text, meters, title } = primaryRemaining(snap);
    return { plugin, snap, text, meters, title };
  });

  const alpha = Math.min(1, Math.max(0, opacity / 100));
  const cols = Math.max(1, cells.length);

  async function openFlyout() {
    await invoke("show_flyout");
    await emit("open-flyout");
  }

  async function openSettings() {
    await invoke("show_flyout");
    await emit("open-settings");
  }

  async function hideBar() {
    await invoke("set_overlay_visible", { visible: false });
    await emit("overlay-toggled", false);
  }

  async function showContextMenu(e: ReactMouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    const menu = await Menu.new({
      items: [
        {
          id: "settings",
          text: "Settings",
          action: () => {
            void openSettings();
          },
        },
        {
          id: "details",
          text: "Open details",
          action: () => {
            void openFlyout();
          },
        },
        {
          id: "hide",
          text: "Hide top bar",
          action: () => {
            void hideBar();
          },
        },
      ],
    });
    await menu.popup();
  }

  async function minimizeBar(e: ReactMouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    await hideBar();
  }

  const minimizeBtn = (
    <button
      type="button"
      className="status-minimize"
      data-no-drag
      title="Hide top bar"
      aria-label="Hide top bar"
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => void minimizeBar(e)}
    >
      <span aria-hidden>─</span>
    </button>
  );

  if (cells.length === 0) {
    return (
      <div
        className="status-bar drag-surface status-bar-empty"
        style={{ background: `rgba(12, 14, 18, ${alpha})` }}
        onMouseDown={beginWindowDrag}
        onDoubleClick={() => void openFlyout()}
        onContextMenu={(e) => void showContextMenu(e)}
        title="Enable a provider in Settings"
      >
        <span className="status-empty-label">No providers enabled</span>
        {minimizeBtn}
      </div>
    );
  }

  return (
    <div
      className="status-bar drag-surface"
      style={{
        background: `rgba(12, 14, 18, ${alpha})`,
        gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr)) auto`,
      }}
      onMouseDown={beginWindowDrag}
      onDoubleClick={() => void openFlyout()}
      onContextMenu={(e) => void showContextMenu(e)}
      title="Drag to move · Double-click details · Right-click menu"
    >
      {cells.map(({ plugin, snap, text, meters, title }) => {
        const accent = plugin.accentColor;
        const ok = snap?.status === "ok";
        const showInlinePct = ok && meters.length > 0;
        return (
          <div
            key={plugin.id}
            className={`status-cell ${ok ? "is-ok" : "is-muted"} ${showInlinePct ? "has-inline-pct" : ""}`}
            title={showInlinePct ? undefined : (title ?? plugin.displayName)}
          >
            <span className="status-dot" style={{ background: accent }} />
            <span className="status-name">{plugin.displayName}</span>
            <div className="status-meters">
              {(showInlinePct ? meters : [null]).map((window, i) => {
                const used = window?.usedPercent ?? null;
                const remaining =
                  used != null ? Math.max(0, 100 - used) : null;
                return (
                  <div
                    key={window?.id ?? i}
                    className="status-meter"
                    data-no-drag
                    title={window ? meterTooltip(window) : undefined}
                  >
                    <div
                      className="status-meter-fill"
                      style={{
                        width:
                          used != null
                            ? `${Math.min(100, Math.max(0, used))}%`
                            : "0%",
                        background: windowBarColor(i, accent),
                        opacity: ok ? 1 : 0.35,
                      }}
                    />
                    {remaining != null && (
                      <span
                        className={`status-meter-label mono ${
                          used != null && used >= 50 ? "is-on-fill" : "is-on-track"
                        }`}
                      >
                        {remaining.toFixed(0)}%
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
            {!showInlinePct && (
              <span className="status-value mono">{text}</span>
            )}
          </div>
        );
      })}
      {minimizeBtn}
    </div>
  );
}
