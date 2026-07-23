import type { ProviderPlugin } from "../types";

export const geminiPlugin: ProviderPlugin = {
  id: "gemini",
  displayName: "Gemini",
  accentColor: "#4a90e2",
  enabledByDefault: false,
  description:
    "Reads Gemini CLI OAuth (~/.gemini/oauth_creds.json) or an access-token override. Quota via Code Assist retrieveUserQuota (unofficial).",
  auth: {
    kind: "hybrid",
    local: {
      kind: "local_session",
      detectLabel: "Auto-detect Gemini CLI OAuth session",
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
          key: "projectId",
          label: "Cloud project ID (optional)",
          secret: false,
          placeholder: "e.g. gen-lang-client-…",
        },
      ],
    },
  },
};
