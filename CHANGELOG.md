# Changelog

All notable changes to HeadRoom are documented in this file.

## [0.4.6] - 2026-10-11

### Changed

- Restore the missing changelog entries for v0.4.1 through v0.4.4, correct the v0.4.0 release date, and link each version to its GitHub release.

## [0.4.5] - 2026-10-11

### Fixed

- Keep the overlay on screen across monitors with different display scaling. Saved positions now use physical desktop coordinates, with recovery after dragging, display disconnection, resolution changes, and resizing. The bar fits inside the selected monitor's work area, clear of the taskbar.

## [0.4.4] - 2026-09-22

### Changed

- Show the remaining Codex credit count beside rate-limit meters in the top bar when purchased credits are available.
- Keep the Credits row in the details flyout and show each rate-limit window's reset countdown.
- Prefer absolute Codex reset timestamps, with relative reset seconds as a fallback.

## [0.4.3] - 2026-08-25

### Added

- Hold Shift to keep the top bar visible and draggable while "Hide when mouse is near" is enabled.
- Settings help text explains the Shift override.

## [0.4.2] - 2026-08-17

### Added

- Show the app version and build date in the Settings header.

## [0.4.1] - 2026-08-17

### Fixed

- Show Codex credits whenever a balance is present, even if the API omits the `has_credits` flag. Compute a usage bar when a total or limit is available.
- Keep credit and budget windows visible when a provider reports three or more rate-limit windows.

## [0.4.0] - 2026-08-17

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

[0.4.6]: https://github.com/mmartain/HeadRoom/releases/tag/v0.4.6
[0.4.5]: https://github.com/mmartain/HeadRoom/releases/tag/v0.4.5
[0.4.4]: https://github.com/mmartain/HeadRoom/releases/tag/v0.4.4
[0.4.3]: https://github.com/mmartain/HeadRoom/releases/tag/v0.4.3
[0.4.2]: https://github.com/mmartain/HeadRoom/releases/tag/v0.4.2
[0.4.1]: https://github.com/mmartain/HeadRoom/releases/tag/v0.4.1
[0.4.0]: https://github.com/mmartain/HeadRoom/releases/tag/v0.4.0
[0.3.0]: https://github.com/mmartain/HeadRoom/releases/tag/v0.3.0
[0.2.2]: https://github.com/mmartain/HeadRoom/releases/tag/v0.2.2
[0.2.1]: https://github.com/mmartain/HeadRoom/releases/tag/v0.2.1
[0.2.0]: https://github.com/mmartain/HeadRoom/releases/tag/v0.2.0
[0.1.1]: https://github.com/mmartain/HeadRoom/releases/tag/v0.1.1
[0.1.0]: https://github.com/mmartain/HeadRoom/releases/tag/v0.1.0
