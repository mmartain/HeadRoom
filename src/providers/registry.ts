import { cursorPlugin } from "./cursor";
import { codexPlugin } from "./codex";
import { claudePlugin } from "./claude";
import { geminiPlugin } from "./gemini";
import { devinPlugin } from "./devin";
import { minimaxPlugin } from "./minimax";
import type { ProviderId, ProviderPlugin } from "./types";

/**
 * Sole concrete registration site.
 * Add providers by creating providers/<id>/ and appending here + a Rust match arm.
 * UI, poller, and alerts stay unchanged.
 */
const ALL_PLUGINS: ProviderPlugin[] = [
  cursorPlugin,
  codexPlugin,
  claudePlugin,
  geminiPlugin,
  devinPlugin,
  minimaxPlugin,
];

export function listPlugins(): ProviderPlugin[] {
  return [...ALL_PLUGINS];
}

export function getPlugin(id: ProviderId): ProviderPlugin | undefined {
  return ALL_PLUGINS.find((p) => p.id === id);
}

export function defaultEnabledMap(): Record<string, boolean> {
  return Object.fromEntries(ALL_PLUGINS.map((p) => [p.id, p.enabledByDefault]));
}

/** Providers the user currently wants polled / shown in glance + overlay. */
export function listEnabledPlugins(enabled: Record<string, boolean>): ProviderPlugin[] {
  return ALL_PLUGINS.filter((p) => enabled[p.id] === true);
}
