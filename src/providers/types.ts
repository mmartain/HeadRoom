/** Provider id is an open string so Claude (etc.) can register later without core changes. */
export type ProviderId = string;

export type AuthField = {
  key: string;
  label: string;
  secret: boolean;
  placeholder?: string;
};

export type AuthCapability =
  | { kind: "local_session"; detectLabel: string }
  | { kind: "secret"; fields: AuthField[] }
  | {
      kind: "hybrid";
      local: { kind: "local_session"; detectLabel: string };
      override: { kind: "secret"; fields: AuthField[] };
    };

export type UsageWindow = {
  id: string;
  label: string;
  usedPercent: number | null;
  remainingLabel: string | null;
  resetsAt: string | null;
};

export type SnapshotStatus = "ok" | "needs_auth" | "error" | "disabled";

export type UsageSnapshot = {
  providerId: ProviderId;
  displayName: string;
  status: SnapshotStatus;
  windows: UsageWindow[];
  fetchedAt: string;
  errorMessage?: string;
};

export type AuthContext = {
  getSecret: (fieldKey: string) => Promise<string | null>;
};

export type ResolvedAuth = {
  source: "local" | "secret" | "none";
};

export type AuthResolution =
  | { ok: true; auth: ResolvedAuth }
  | { ok: false; reason: string };

export type FetchContext = {
  signal?: AbortSignal;
};

/**
 * Plugin contract — UI/core depend only on this interface.
 * Concrete vendors live under providers/<id>/ and register in registry.ts.
 */
export interface ProviderPlugin {
  id: ProviderId;
  displayName: string;
  accentColor: string;
  auth: AuthCapability;
  enabledByDefault: boolean;
  /** Optional short blurb shown in Settings. */
  description?: string;
}
