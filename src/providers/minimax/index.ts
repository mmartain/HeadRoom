import type { ProviderPlugin } from "../types";

export const minimaxPlugin: ProviderPlugin = {
  id: "minimax",
  displayName: "MiniMax",
  accentColor: "#7c5cff",
  enabledByDefault: false,
  description:
    "Queries your MiniMax Token Plan via API key (platform.minimax.io → console → plan). Uses the unofficial /v1/token_plan/remains endpoint.",
  auth: {
    kind: "secret",
    fields: [
      {
        key: "apiKey",
        label: "API key",
        secret: true,
        placeholder: "Paste your MiniMax API key",
      },
      {
        key: "baseUrl",
        label: "Base URL (optional)",
        secret: false,
        placeholder: "https://www.minimax.io (CN: https://api.minimaxi.com)",
      },
    ],
  },
};
