import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listPlugins } from "../providers/registry";
import type { AuthCapability, AuthField, ProviderPlugin } from "../providers/types";
import type { AppSettings } from "../store/snapshots";

type Props = {
  settings: AppSettings;
  onChange: (next: AppSettings) => void;
  onOpacityLive: (opacity: number) => void;
  onClose: () => void;
};

function collectSecretFields(auth: AuthCapability): AuthField[] {
  if (auth.kind === "secret") return auth.fields;
  if (auth.kind === "hybrid") return auth.override.fields;
  return [];
}

function ProviderSettings({ plugin }: { plugin: ProviderPlugin }) {
  const fields = useMemo(() => collectSecretFields(plugin.auth), [plugin]);
  const [values, setValues] = useState<Record<string, string>>({});
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const next: Record<string, string> = {};
      for (const f of fields) {
        const v = await invoke<string | null>("get_secret", {
          providerId: plugin.id,
          fieldKey: f.key,
        });
        next[f.key] = v ?? "";
      }
      if (!cancelled) setValues(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [fields, plugin.id]);

  async function saveField(field: AuthField, value: string) {
    await invoke("set_secret", {
      providerId: plugin.id,
      fieldKey: field.key,
      value,
    });
    setSaved(true);
    window.setTimeout(() => setSaved(false), 1500);
  }

  return (
    <section className="settings-provider">
      <h3>{plugin.displayName}</h3>
      {plugin.description && <p className="muted">{plugin.description}</p>}
      {plugin.auth.kind === "local_session" || plugin.auth.kind === "hybrid" ? (
        <p className="detect-label">
          {(plugin.auth.kind === "hybrid" ? plugin.auth.local : plugin.auth).detectLabel}
        </p>
      ) : null}
      {fields.map((field) => (
        <label key={field.key} className="field">
          <span>{field.label}</span>
          <div className="field-row">
            <input
              type={field.secret ? "password" : "text"}
              value={values[field.key] ?? ""}
              placeholder={field.placeholder}
              autoComplete="off"
              spellCheck={false}
              onChange={(e) =>
                setValues((prev) => ({ ...prev, [field.key]: e.target.value }))
              }
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void saveField(field, (e.target as HTMLInputElement).value);
                  (e.target as HTMLInputElement).blur();
                }
              }}
              onBlur={(e) => void saveField(field, e.target.value)}
            />
            <button
              type="button"
              className="save-field"
              onClick={() => void saveField(field, values[field.key] ?? "")}
            >
              Save
            </button>
          </div>
        </label>
      ))}
      {saved && <p className="saved-hint">Saved</p>}
    </section>
  );
}

export function SettingsForm({ settings, onChange, onOpacityLive, onClose }: Props) {
  const plugins = listPlugins();

  return (
    <div className="settings">
      <header className="settings-header">
        <h2>Settings</h2>
        <button type="button" className="linkish" onClick={onClose}>
          Back
        </button>
      </header>

      <div className="settings-body">
        <section className="settings-block">
          <h3>Providers</h3>
          {plugins.map((plugin) => (
            <label key={plugin.id} className="toggle-row">
              <span>{plugin.displayName}</span>
              <input
                type="checkbox"
                checked={settings.enabled[plugin.id] === true}
                onChange={(e) =>
                  onChange({
                    ...settings,
                    enabled: { ...settings.enabled, [plugin.id]: e.target.checked },
                  })
                }
              />
            </label>
          ))}
        </section>

        <section className="settings-block">
          <h3>Top status bar</h3>
          <label className="toggle-row">
            <span>Show top status bar</span>
            <input
              type="checkbox"
              checked={settings.overlayVisible}
              onChange={(e) => onChange({ ...settings, overlayVisible: e.target.checked })}
            />
          </label>
          <label className="toggle-row">
            <span>Hide when mouse is near</span>
            <input
              type="checkbox"
              checked={settings.overlayHideNearMouse}
              onChange={(e) =>
                onChange({ ...settings, overlayHideNearMouse: e.target.checked })
              }
            />
          </label>
          <label className="field">
            <span>Transparency / opacity ({settings.overlayOpacity}%)</span>
            <input
              type="range"
              min={15}
              max={100}
              step={1}
              value={settings.overlayOpacity}
              onInput={(e) => onOpacityLive(Number((e.target as HTMLInputElement).value))}
              onChange={(e) => onOpacityLive(Number(e.target.value))}
            />
          </label>
          <p className="muted detect-label">
            When “Hide when mouse is near” is on, the bar ducks away so you can click
            title bars and tabs underneath. Tray: “Toggle top status bar”.
          </p>
        </section>

        <section className="settings-block">
          <h3>Polling</h3>
          <label className="field">
            <span>Interval (seconds)</span>
            <input
              type="number"
              min={30}
              max={600}
              value={settings.pollIntervalSec}
              onChange={(e) =>
                onChange({
                  ...settings,
                  pollIntervalSec: Math.max(30, Number(e.target.value) || 120),
                })
              }
            />
          </label>
        </section>

        <section className="settings-block">
          <h3>Credentials</h3>
          <p className="muted detect-label">
            Values save when you leave a field. Paste a Devin service key here to connect.
          </p>
          {plugins.map((plugin) => (
            <ProviderSettings key={plugin.id} plugin={plugin} />
          ))}
        </section>
      </div>
    </div>
  );
}
