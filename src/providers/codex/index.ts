import type { ProviderPlugin } from "../types";

export const codexPlugin: ProviderPlugin = {
  id: "codex",
  displayName: "Codex",
  accentColor: "#10a37f",
  enabledByDefault: true,
  description: "Reads ~/.codex/auth.json from ChatGPT Codex login, or an override token.",
  auth: {
    kind: "hybrid",
    local: {
      kind: "local_session",
      detectLabel: "Auto-detect Codex / ChatGPT login",
    },
    override: {
      kind: "secret",
      fields: [
        {
          key: "accessToken",
          label: "Access token override",
          secret: true,
          placeholder: "Optional Bearer token",
        },
        {
          key: "accountId",
          label: "ChatGPT account id (optional)",
          secret: false,
          placeholder: "ChatGPT-Account-Id header if needed",
        },
      ],
    },
  },
};
