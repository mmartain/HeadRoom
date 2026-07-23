# Security

## Reporting a vulnerability

Please open a **private** GitHub security advisory on this repository, or email the maintainer via GitHub. Do not file a public issue for credential leaks or remote-code risks.

## What HeadRoom stores

- Settings and optional pasted secrets live only under `%APPDATA%\headroom\` on your machine.
- HeadRoom never syncs credentials to a HeadRoom cloud service.
- Provider session tokens are read from local app data (Cursor, Devin, Codex, Claude, Gemini) when present.

## Scope notes

- Provider usage/quota APIs used here are **unofficial** and may change or break without notice.
- Bundled OAuth client IDs/secrets match those used by the respective official CLIs/desktop apps (installed-app style clients). They are not HeadRoom user secrets.
- Treat any pasted API/session keys in Settings as sensitive; do not commit `secrets.json` or share screenshots that include them.
