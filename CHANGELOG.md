# Changelog

All notable changes to Devti. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Phase 3 — Control
#### Added
- Protocol: input events (pointer, keys, text, media, touch gestures, navigation), system
  actions, app launch, saved commands with results, shortcuts-deck presses, catalog exchange,
  and live screens (adaptive JPEG frames with acks, latency and fps both ways). Every request is
  checked against the receiver's permissions.
- Hub:
  - input injection with `enigo`
  - lock, sleep, shut down, restart, sign out, volume and brightness
  - installed apps
  - saved commands with a 60-second limit and output
  - shortcuts deck with hotkeys, media keys, apps, commands, text, links and OBS scenes
  - **Control** page with the deck editor, saved commands and the local API
  - device control panel, and **Mirror and control** for Android phones with screenshots and
    recordings
- Local API on 127.0.0.1 with a bearer token, plus the `devti` CLI (`send`, `ring`, `clip`,
  `run`, `watch`), packaged in deb/rpm and shipped beside the Windows installers.
- Android:
  - **Remote** tab: touchpad, keyboard, keys, media, sliders, power, apps, commands and a
    presenter
  - **Deck** tab
  - screen sharing with MediaProjection, and the **Devti control** accessibility service for
    taps, swipes, typing and navigation
  - flashlight, screenshot and camera commands
- Tests: control and screen end-to-end tests with every permission boundary; a CLI test against
  a real node (auth, DNS-rebinding guard, files, commands, events); host unit tests (hotkeys,
  desktop entries, OBS auth vector, commands); 2 new UI flows; Kotlin binding test for the
  catalog, input and commands; the real-app smoke test runs a saved command from the phone.

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
