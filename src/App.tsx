import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { Flyout } from "./components/Flyout";
import { Overlay } from "./components/Overlay";
import { SettingsForm } from "./components/SettingsForm";
import type { UsageSnapshot } from "./providers/types";
import {
  AlertTracker,
  fetchSnapshots,
  loadLastResets,
  loadSettings,
  mergeSettings,
  saveLastResets,
  saveSettings,
  worstRemainingPercent,
  type AppSettings,
} from "./store/snapshots";
import { ResetTracker } from "./store/resetTracker";
import "./App.css";

type View = "flyout" | "settings";

function mergePartial(payload: Record<string, unknown>): Partial<AppSettings> {
  const merged = mergeSettings(payload);
  return {
    overlayVisible: merged.overlayVisible,
    overlayOpacity: merged.overlayOpacity,
    overlayZoom: merged.overlayZoom,
    overlayHideNearMouse: merged.overlayHideNearMouse,
    enabled: merged.enabled,
    pollIntervalSec: merged.pollIntervalSec,
    alertThresholds: merged.alertThresholds,
    notifyOnReset: merged.notifyOnReset,
  };
}

function useWindowLabel(): string {
  const [label, setLabel] = useState("main");
  useEffect(() => {
    setLabel(getCurrentWindow().label);
  }, []);
  return label;
}

async function ensureNotifyPermission(): Promise<boolean> {
  let granted = await isPermissionGranted();
  if (!granted) {
    const perm = await requestPermission();
    granted = perm === "granted";
  }
  return granted;
}

export default function App() {
  const windowLabel = useWindowLabel();
  const isTopBar = windowLabel === "overlay";
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [snapshots, setSnapshots] = useState<UsageSnapshot[]>([]);
  const [loading, setLoading] = useState(false);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);
  const [view, setView] = useState<View>("flyout");
  const alerts = useRef(new AlertTracker());
  const resets = useRef(new ResetTracker());
  const resetsSeeded = useRef(false);
  const saveTimer = useRef<number | null>(null);
  const settingsRef = useRef<AppSettings | null>(null);
  const shellRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);

  // Size the tray flyout to its content; Settings keeps a taller fixed panel.
  useEffect(() => {
    if (isTopBar) return;

    if (view === "settings") {
      void invoke("fit_flyout_size", { height: 560 });
      return;
    }

    const el = shellRef.current;
    if (!el) return;

    let frame = 0;
    const apply = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const h = Math.ceil(el.getBoundingClientRect().height);
        if (h > 0) void invoke("fit_flyout_size", { height: h });
      });
    };

    apply();
    const ro = new ResizeObserver(apply);
    ro.observe(el);
    return () => {
      cancelAnimationFrame(frame);
      ro.disconnect();
    };
  }, [isTopBar, view, snapshots, settings?.enabled]);

  const applySnapshots = useCallback((next: UsageSnapshot[]) => {
    setSnapshots(next);
    setLastUpdated(new Date());
  }, []);

  const refresh = useCallback(
    async (enabled: Record<string, boolean>, opts?: { broadcastOnly?: boolean }) => {
      setLoading(true);
      try {
        const next = await fetchSnapshots(enabled);
        applySnapshots(next);
        await emit("snapshots-updated", next);
        if (opts?.broadcastOnly) return;

        await invoke("update_tray_status", {
          worstRemaining: worstRemainingPercent(next),
        });

        const s = await loadSettings();
        const hits = alerts.current.evaluate(next, s.alertThresholds);
        if (hits.length > 0 && (await ensureNotifyPermission())) {
          for (const hit of hits) {
            sendNotification({
              title: `${hit.displayName} usage alert`,
              body: `${hit.windowLabel} reached ${hit.usedPercent.toFixed(0)}% (threshold ${hit.threshold}%).`,
            });
          }
        }

        const resetHits = resets.current.evaluate(next);
        if (resetHits.length > 0 && s.notifyOnReset && (await ensureNotifyPermission())) {
          for (const hit of resetHits) {
            sendNotification({
              title: `${hit.displayName} limits reset`,
              body: `${hit.windowLabel} window refreshed${hit.usedPercent != null ? ` — ${hit.usedPercent.toFixed(0)}% used` : ""}.`,
            });
          }
        }
        // Persist unconditionally (even when the toggle is off) so the
        // baseline stays fresh across restarts; prunes disabled providers.
        await saveLastResets(resets.current.snapshot(enabled));
      } catch (err) {
        console.error(err);
      } finally {
        setLoading(false);
      }
    },
    [applySnapshots],
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const s = await loadSettings();
      if (cancelled) return;
      setSettings(s);
      if (isTopBar) {
        if (s.overlayVisible) {
          await invoke("set_overlay_visible", { visible: true });
        }
        // Top bar follows main window broadcasts; ask for a fresh pull once.
        await emit("request-refresh", null);
        return;
      }
      // Seed the reset tracker from last session so a reset that happened
      // while the app was closed fires on the first poll.
      if (!resetsSeeded.current) {
        resets.current.seed(await loadLastResets());
        resetsSeeded.current = true;
      }
      await refresh(s.enabled);
    })();
    return () => {
      cancelled = true;
    };
  }, [refresh, isTopBar]);

  // Only the main (flyout) window owns the poll loop — avoids Cursor DB races
  // and keeps the top bar in sync via snapshots-updated.
  useEffect(() => {
    if (isTopBar || !settings) return;
    const ms = settings.pollIntervalSec * 1000;
    const id = window.setInterval(() => {
      void refresh(settings.enabled);
    }, ms);
    return () => window.clearInterval(id);
  }, [isTopBar, settings?.pollIntervalSec, settings?.enabled, refresh]);

  useEffect(() => {
    const unlistenRefresh = listen("tray-refresh", () => {
      if (isTopBar) return;
      const current = settingsRef.current;
      if (current) void refresh(current.enabled);
    });
    const unlistenRequest = listen("request-refresh", () => {
      if (isTopBar) return;
      const current = settingsRef.current;
      if (current) void refresh(current.enabled);
    });
    const unlistenSnapshots = listen<UsageSnapshot[]>("snapshots-updated", (ev) => {
      applySnapshots(ev.payload);
    });
    const unlistenOverlay = listen<boolean>("overlay-toggled", (ev) => {
      setSettings((prev) => (prev ? { ...prev, overlayVisible: ev.payload } : prev));
    });
    const unlistenOpenSettings = listen("open-settings", () => {
      if (isTopBar) return;
      setView("settings");
    });
    const unlistenOpenFlyout = listen("open-flyout", () => {
      if (isTopBar) return;
      setView("flyout");
    });
    const unlistenSettings = listen<Record<string, unknown>>("settings-changed", (ev) => {
      setSettings((prev) => {
        if (!prev) return prev;
        return { ...prev, ...mergePartial(ev.payload) };
      });
    });
    return () => {
      void unlistenRefresh.then((u) => u());
      void unlistenRequest.then((u) => u());
      void unlistenSnapshots.then((u) => u());
      void unlistenOverlay.then((u) => u());
      void unlistenOpenSettings.then((u) => u());
      void unlistenOpenFlyout.then((u) => u());
      void unlistenSettings.then((u) => u());
    };
  }, [refresh, applySnapshots, isTopBar]);

  async function updateSettings(next: AppSettings) {
    const prev = settingsRef.current;
    setSettings(next);
    await saveSettings(next);
    await emit("settings-changed", next);
    await invoke("set_overlay_visible", { visible: next.overlayVisible });
    if (next.overlayVisible && prev?.overlayZoom !== next.overlayZoom) {
      await invoke("refresh_overlay_layout", { zoom: next.overlayZoom });
    }
    const providersChanged =
      !prev || JSON.stringify(prev.enabled) !== JSON.stringify(next.enabled);
    if (providersChanged) {
      await refresh(next.enabled);
    }
  }

  function liveOpacity(opacity: number) {
    const current = settingsRef.current;
    if (!current) return;
    const next = { ...current, overlayOpacity: opacity };
    setSettings(next);
    void emit("settings-changed", next);
    if (saveTimer.current != null) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      void saveSettings(next);
    }, 250);
  }

  function liveZoom(zoom: number) {
    const current = settingsRef.current;
    if (!current) return;
    const next = { ...current, overlayZoom: zoom };
    // Update settings UI immediately; resize the native window first, then
    // broadcast so the overlay webview scales after the new size is applied.
    setSettings(next);
    void (async () => {
      await invoke("refresh_overlay_layout", { zoom });
      await emit("settings-changed", next);
    })();
    if (saveTimer.current != null) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      void saveSettings(next);
    }, 250);
  }

  async function toggleOverlay() {
    if (!settings) return;
    const nextVisible = !settings.overlayVisible;
    const next = { ...settings, overlayVisible: nextVisible };
    setSettings(next);
    await saveSettings(next);
    await invoke("set_overlay_visible", { visible: nextVisible });
    await emit("settings-changed", next);
    if (nextVisible) {
      await invoke("hide_flyout");
      // Push latest numbers to the top bar immediately
      await refresh(next.enabled);
    }
  }

  if (!settings) {
    return <div className="boot">Loading…</div>;
  }

  if (isTopBar) {
    const z = Math.min(150, Math.max(75, settings.overlayZoom)) / 100;
    return (
      <div
        className="overlay-root"
        style={{
          width: `${100 / z}%`,
          height: `${100 / z}%`,
          transform: `scale(${z})`,
          transformOrigin: "top left",
        }}
      >
        <Overlay
          snapshots={snapshots}
          enabled={settings.enabled}
          opacity={settings.overlayOpacity}
        />
      </div>
    );
  }

  return (
    <div
      ref={shellRef}
      className={`app-shell ${view === "settings" ? "app-shell--settings" : "app-shell--flyout"}`}
    >
      {view === "flyout" ? (
        <Flyout
          snapshots={snapshots}
          enabled={settings.enabled}
          lastUpdated={lastUpdated}
          loading={loading}
          onRefresh={() => refresh(settings.enabled)}
          onOpenSettings={() => setView("settings")}
          overlayVisible={settings.overlayVisible}
          onToggleOverlay={() => void toggleOverlay()}
        />
      ) : (
        <SettingsForm
          settings={settings}
          onChange={(next) => void updateSettings(next)}
          onOpacityLive={liveOpacity}
          onZoomLive={liveZoom}
          onClose={() => setView("flyout")}
        />
      )}
    </div>
  );
}
