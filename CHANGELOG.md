# Changelog

All notable changes to Devti. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
