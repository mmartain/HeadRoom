import type { ProviderPlugin } from "../types";

export const cursorPlugin: ProviderPlugin = {
  id: "cursor",
  displayName: "Cursor",
  accentColor: "#e85d04",
  enabledByDefault: true,
  description: "Reads your local Cursor session (state.vscdb) or an override token.",
  auth: {
    kind: "hybrid",
    local: {
      kind: "local_session",
      detectLabel: "Auto-detect Cursor login from local session",
    },
    override: {
      kind: "secret",
      fields: [
        {
          key: "accessToken",
          label: "Access token override",
          secret: true,
          placeholder: "Optional — leave blank to use local session",
        },
      ],
    },
  },
};
