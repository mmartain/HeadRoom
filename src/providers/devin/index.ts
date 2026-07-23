import type { ProviderPlugin } from "../types";

export const devinPlugin: ProviderPlugin = {
  id: "devin",
  displayName: "Devin",
  accentColor: "#3d8bfd",
  enabledByDefault: true,
  description:
    "Reads your Devin desktop login (local session) like Cursor. Optional overrides for a pasted session key or team service key.",
  auth: {
    kind: "hybrid",
    local: {
      kind: "local_session",
      detectLabel: "Auto-detect Devin desktop login from local session",
    },
    override: {
      kind: "secret",
      fields: [
        {
          key: "accessToken",
          label: "Session / API key override",
          secret: true,
          placeholder: "Optional — leave blank to use Devin desktop login",
        },
        {
          key: "serviceKey",
          label: "Team service key (optional)",
          secret: true,
          placeholder: "Only if you need team credit balance instead",
        },
      ],
    },
  },
};
