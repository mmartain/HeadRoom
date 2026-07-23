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
  loadSettings,
  mergeSettings,
  saveSettings,
  worstRemainingPercent,
  type AppSettings,
} from "./store/snapshots";
import "./App.css";

type View = "flyout" | "settings";

function mergePartial(payload: Record<string, unknown>): Partial<AppSettings> {
  const merged = mergeSettings(payload);
  return {
    overlayVisible: merged.overlayVisible,
    overlayOpacity: merged.overlayOpacity,
    overlayHideNearMouse: merged.overlayHideNearMouse,
    enabled: merged.enabled,
    pollIntervalSec: merged.pollIntervalSec,
    alertThresholds: merged.alertThresholds,
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
  const saveTimer = useRef<number | null>(null);
  const settingsRef = useRef<AppSettings | null>(null);

  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);

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

        const hits = alerts.current.evaluate(
          next,
          (await loadSettings()).alertThresholds,
        );
        if (hits.length > 0 && (await ensureNotifyPermission())) {
          for (const hit of hits) {
            sendNotification({
              title: `${hit.displayName} usage alert`,
              body: `${hit.windowLabel} reached ${hit.usedPercent.toFixed(0)}% (threshold ${hit.threshold}%).`,
            });
          }
        }
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
    return (
      <div className="overlay-root">
        <Overlay
          snapshots={snapshots}
          enabled={settings.enabled}
          opacity={settings.overlayOpacity}
        />
      </div>
    );
  }

  return (
    <div className="app-shell">
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
          onClose={() => setView("flyout")}
        />
      )}
    </div>
  );
}
