# Changelog

All notable changes to Devti. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Phase 2 — Continuity
#### Added
- Protocol: clipboard, ring, notification post/remove/action/dismiss, and Do Not Disturb
  messages; a resumable `file` stream verified with SHA-256. Every request is checked against
  the receiver's permissions.
- Hub: **Files** (drop anywhere or on a device, live progress, cancel/retry/open, clipboard
  history), **Alerts** (phone notifications with actions, quick reply, dismiss, DND), **Battery**
  (24 h / 7 d charts, validated colour-blind-safe palette, table view, low-battery alert), and
  *Ring it* with a full-screen overlay and chime. Adds OS clipboard sync, native notifications,
  and `--send` paths for "Send to Devti".
- Android: notification mirroring via a notification listener (actions, RemoteInput replies,
  dismiss, DND in both directions), clipboard sync within Android's limits, find my phone at
  alarm volume over the lock screen, *Send with Devti* in the share sheet, received files in
  Downloads/Devti, and per-computer permissions.
- Tests: core continuity end-to-end suite (including a 48 MB resume after a dropped
  connection), Phase 2 checks in the real-app smoke test, and Kotlin binding tests covering
  clipboard, notifications, a file verified byte for byte, and revoking a permission.

#### Fixed
- File transfers could stall when a device reconnected before the interrupted attempt had
  unwound; the sender now retries by itself.

### Phase 1 — Foundation
#### Added
- **Devti Core** (Rust): Ed25519 device identities with pinned fingerprints; mutual TLS 1.3 over
  QUIC; SPAKE2 pairing bound to the TLS session with a 6-digit verification code; QR pairing
  links; mDNS discovery; SQLite trust store, settings and activity log; automatic reconnection
  with backoff; duplicate-connection resolution; kill switch; revoke on both sides.
- **UniFFI bindings** for the mobile apps.
- **Desktop Hub** (Windows, Linux): "Prism" design system with a custom icon set; seven-step
  first-run wizard; live ecosystem map; pairing by QR or code; device pages with per-feature
  permissions; Security page with kill switch, activity log (CSV export) and identity details;
  Settings (theme, motion, autostart, tray, rename, diagnostics); Help with compatibility table,
  troubleshooting and tour; tray icon; live Bluetooth accessory detection.
- **Android app**: onboarding with CameraX + ZXing QR scanner and code pairing; home screen;
  Keystore-sealed identity; foreground service; battery sharing.
- **Quality**: 69 Rust tests (91 % core line coverage), 3 fuzz targets, 14 UI unit tests,
  7 Playwright flows, a real-app smoke test, Kotlin binding tests on the JVM, CI for Linux,
  Windows and Android.

#### Fixed
- Node tasks no longer assume an ambient Tokio runtime (calls from UI threads panicked).
