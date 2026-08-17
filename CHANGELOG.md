# Changelog

All notable changes to HeadRoom are documented in this file.

## [0.4.0] - 2026-08-16

### Added

- **Launch at startup** — opt-in toggle in Settings → Startup & updates; writes a registry Run key so HeadRoom starts with Windows.
- **Auto-update** — checks GitHub Releases on startup (toggle, default ON). Installed builds download and install the signed NSIS update automatically; portable builds self-replace via a download-and-swap mechanism. New "Check for updates" button and status in Settings. Update-available banner in the flyout.
- **Encrypted secrets** — provider credentials are now encrypted at rest with Windows DPAPI (current-user scope). Existing plaintext `secrets.json` is migrated transparently on first write. No frontend changes needed.

### Changed

- Settings restructured into sections: Startup & updates, Notifications (existing reset toggle), Credentials.
- Release pipeline now signs the NSIS installer and publishes `latest.json` for the updater.

## [0.3.0] - 2026-08-16

### Added

- **Reset notifications** — native Windows notification whenever a usage window resets (Claude 5-hour/weekly, Devin daily/weekly, Cursor monthly, Gemini, Codex). Resets that happened while HeadRoom was closed are caught on the next launch. Each reset is notified exactly once (last-seen timestamps persisted in `last_resets.json`).
- **"Notify when limits reset" toggle** in Settings → Notifications (on by default).
- **MiniMax provider** — optional Token Plan usage tracking via API key (`platform.minimax.io` → console → plan), off by default; enable in Settings → Providers.

## [0.2.2] - 2026-08-13

### Fixed

- Cursor: HeadRoom no longer locks Cursor's state database during polls, so Cursor can restart while HeadRoom is running.

### Changed

- Updated README screenshot.

## [0.2.1] - 2026-07-31

### Added

- Top-bar zoom setting (75–150%).

### Changed

- Improved top-bar label readability (JetBrains Mono labels with adaptive contrast).

## [0.2.0] - 2026-07-27

### Added

- Cursor Models / Other Models / On-Demand usage bars.
- Hardened overlay restore: bar position persists across restarts with on-screen clamping and reliable mouse-duck restore.

## [0.1.1] - 2026-07-23

### Changed

- Responsive UI and layout improvements with dynamic flyout height fitting.
- Vite cache directory moved out of the synced folder to avoid Dropbox issues.

## [0.1.0] - 2026-07-23

### Added

- Initial release: system-tray flyout showing remaining AI coding usage for Cursor, ChatGPT Codex, Claude, Gemini, and Devin.
- Optional always-on-top floating overlay (enabled providers only).
- Threshold toasts at 80% / 95% used.
- Modular provider plugin design; single-instance portable EXE.
