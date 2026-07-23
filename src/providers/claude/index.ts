import type { ProviderPlugin } from "../types";

export const claudePlugin: ProviderPlugin = {
  id: "claude",
  displayName: "Claude",
  accentColor: "#d97757",
  enabledByDefault: false,
  description:
    "Reads Claude Code login (~/.claude/.credentials.json) or an OAuth access token override. Uses the same unofficial /api/oauth/usage endpoint as Claude Code.",
  auth: {
    kind: "hybrid",
    local: {
      kind: "local_session",
      detectLabel: "Auto-detect Claude Code OAuth session",
    },
    override: {
      kind: "secret",
      fields: [
        {
          key: "accessToken",
          label: "Access token override",
          secret: true,
          placeholder: "Optional — leave blank to use Claude Code login",
        },
      ],
    },
  },
};
