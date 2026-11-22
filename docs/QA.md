# Quality assurance log

Every phase ends with the full test suite, a manual walk through that phase's checklist, and
the results recorded here. A phase is only marked done when all of this passes.

---

## Phase 2 — Continuity

**Date:** 2026-11-22

### Automated results

| Suite | Result | Where it runs |
|---|---|---|
| Core unit tests | 63 / 63 pass | local + CI (Linux, Windows) |
| Core end-to-end, Phase 1 | 12 / 12 pass | local + CI |
| Core end-to-end, continuity | 6 / 6 pass — clipboard both ways + echo suppression + permission refusal, ring, notification post/action/reply/dismiss/removal + clearing on disconnect, DND propagation, battery history + low-battery alert once, **a 48 MB file resumed after a dropped connection and verified byte for byte**, cancel | local + CI |
| Clippy (`-D warnings`), rustfmt | clean | local + CI |
| Desktop unit tests (Vitest) | 18 / 18 pass | local + CI |
| Desktop UI flows (Playwright) | 12 / 12 pass (adds files, notifications + reply + DND, battery chart + table view, ring overlay) | local + CI |
| **Real-app smoke test, Phase 2** | **pass** — real Hub binary + simulated phone over QUIC: phone notification mirrored, 3 MiB file received and complete, clipboard reached the phone, ring reached the phone, quick reply reached the phone, kill switch | local + CI (Linux) |
| Android: Kotlin bindings vs real core | pass — adds clipboard, notification publish → hub, a 300 KB file hub → phone verified byte for byte, and revoking the hub's clipboard permission from the phone | local + CI |
| Android: app unit tests | reducer (5) + pure helpers (3) | CI |
| Android: release APK | built in CI | CI |

### Bugs found and fixed during Phase 2 QA
- **Transfer resume race:** if the device reconnected before the interrupted attempt had
  unwound, the resume was skipped and the transfer sat at "paused". Fixed with a self-retry.
  Covered by the 48 MB resume test.
- **Ring overlay:** the animated halo intercepted the click on "I found it". It's now
  `pointer-events: none`, and the Playwright test clicks the real button.
- **Battery chart palette:** the first colour steps failed the colour-blindness separation and
  dark-mode lightness checks. The chart now uses its own validated steps (worst CVD ΔE 15.9),
  plus direct labels and a table view.
- **Browser preview confusion** (reported by the user): the web preview paired a simulated
  "Pixel 8" by itself. The preview now carries a permanent banner and simulates only on request.

### Manual checklist (Phase 2)
- [x] Hub: drop a file on the window and on a device node, watch progress, cancel, retry, open,
  show in folder (preview + real binary).
- [x] Hub: copy text on one side, paste on the other; history; turn sync off.
- [x] Hub: notification actions, quick reply, dismiss, DND toggle (real binary via the smoke
  test; UI via Playwright).
- [x] Hub: Ring it → phone receives; incoming ring shows the overlay and chime until "I found it".
- [x] Battery: 24 h / 7 d, hover readout, table view, light and dark themes.
- [ ] Android on a real phone: notification access flow, RemoteInput replies in real messaging
  apps, alarm-volume ring over the lock screen, share sheet, Downloads/Devti. *(Needs hardware
  — see below.)*

### Not verifiable in the build environment
- Everything that needs an Android device: notification listener, RemoteInput, full-screen
  intents (Android 14+ may show a heads-up instead), MediaStore, clipboard focus rules,
  vibration. The code paths behind these are typed and compiled in CI, and their
  platform-free logic is unit-tested.
- Native OS notifications and clipboard on real Windows and GNOME/KDE sessions. The smoke test
  runs under Xvfb, so arboard uses X11 there.

---

## Phase 1 — Foundation

**Date:** 2026-11-21 · **Commit:** see the branch history for the run that produced the artifacts

### Automated results

| Suite | Result | Where it runs |
|---|---|---|
| Core unit tests | 57 / 57 pass | local + CI (Linux, Windows) |
| Core end-to-end over real QUIC | 12 / 12 pass (QR + code pairing, wrong-code lockout, MITM pin refusal, rejection, revoke on both sides, reconnect after restart, kill switch, permissions, off-runtime calls) | local + CI (Linux, Windows) |
| FFI (mobile bridge) | 1 / 1 pass | local + CI |
| Desktop host (Rust) | 3 / 3 pass (log redaction, names, Bluetooth labels) | local + CI |
| **Core line coverage** | **91.4 %** (target ≥ 80 %, enforced in CI) | local + CI |
| Fuzzing | 3 targets, ~2.3 M local executions + 60 s each in CI, **0 crashes** | local + CI |
| Clippy (`-D warnings`) | clean on all crates | local + CI |
| Desktop lint (Biome) + TypeScript strict | clean | local + CI (Linux, Windows) |
| Desktop unit tests (Vitest) | 14 / 14 pass | local + CI |
| Desktop UI flows (Playwright) | 7 / 7 pass (full wizard, device permissions/roles/removal, kill switch, theme + motion, tour, keyboard rail, computer pairing) | local + CI |
| **Real-app smoke test** | **pass** — WebDriver drives the actual WebKitGTK Hub binary through onboarding, pairs it over real QUIC with a simulated phone, checks battery sync and the kill switch | local + CI (Linux) |
| Android: Kotlin bindings vs real core | 3 / 3 pass — two Rust nodes paired through the generated Kotlin API + state reducer on the JVM | local + CI |
| Android: app unit tests | 3 / 3 (reducer) | CI |
| Android: release APK | built in CI (arm64-v8a, armeabi-v7a, x86_64) | CI |
| Desktop installers | NSIS + MSI (Windows), deb + rpm + AppImage (Linux) | CI |

### Bugs found and fixed during QA
| Bug | Found by | Fix |
|---|---|---|
| Opening a pairing window from a Tauri sync command panicked ("no reactor running") | real-app smoke test | Node captures its Tokio runtime handle at start and spawns every task on it; regression test `api_is_callable_from_threads_outside_the_runtime` |
| Pairing IDs above 2⁵³ would be rounded by JavaScript, so "codes match" could target the wrong attempt | review while wiring the Tauri bridge | IDs masked below 2⁵³ in core; test added |
| UniFFI error field `message` collides with Kotlin `Throwable.message` | inspecting generated bindings | field renamed to `reason` |
| Zustand selectors returning new arrays caused an infinite render loop on the pairing sheet | Playwright screenshot run | select stable state, derive with `useMemo` |
| Security page rows lost their layout (`all: unset` overrode the grid) | visual review | targeted resets instead of `all: unset` |
| Windows CI lint failed on CRLF checkouts | CI | `.gitattributes` forces LF |
| Release APK signing needed the debug keystore before it existed | CI | build debug first; real releases use a keystore secret |

### Performance
Measured on Linux (Xvfb, software rendering, no GPU), sampling CPU time and PSS memory (which
counts shared libraries fairly) across the Hub and its WebKit helper processes.

| State | CPU | Memory (PSS) | Target |
|---|---|---|---|
| Running in the background, window closed (autostart / tray) — release build | **0.03 %** of one core | **89 MB** | CPU < 2 %, RAM < 250 MB ✅ |
| Window open on the dashboard, nobody interacting (paired device, idle) — debug build | **0.13 %** of one core | **245 MB** (host 108 MB, renderer 115 MB, network 21 MB) | ✅ (release host is ~20 MB smaller) |
| Window open while being used (ambient motion running) | ~21 % of one core with *software* rendering | — | not an idle state; GPU compositing on real desktops is far cheaper |

What made the difference: ambient motion (prism breathing, light pulses) now **rests after 15 s
without input** or whenever the window is hidden; it used to run forever (~9 % CPU idle). The
prism breathing also moved from a JavaScript-driven animation to a CSS transform. Closing the
window destroys the webview entirely, which is why the background state needs only 89 MB.

Screen-extension latency (< 50 ms) is a Phase 4 target and will be measured there.

### Manual checklist (Phase 1)

| # | Check | Result |
|---|---|---|
| 1 | Fresh install shows the onboarding wizard, never a blank screen | ✅ (real app, WebKitGTK) |
| 2 | Welcome animation plays; with reduced motion it is static | ✅ |
| 3 | Permissions step explains all five permissions with platform-specific notes | ✅ |
| 4 | Renaming in step 3 updates the Hub everywhere | ✅ |
| 5 | QR code renders and is scannable (dark modules on a light card in both themes) | ✅ |
| 6 | Phone pairs from the QR; both sides show the same 6-digit code | ✅ (simulated phone via real QUIC) |
| 7 | "They're different" on either side aborts on both | ✅ (core test) |
| 8 | Three wrong codes close the pairing window | ✅ (core test) |
| 9 | A forged QR (wrong fingerprint) is refused by TLS | ✅ (core test) |
| 10 | Paired device appears on the map with its color, battery ring and thread | ✅ |
| 11 | Device going offline drifts outward with a dashed thread and "Seen …" | ✅ (preview + core events) |
| 12 | Restarting a device reconnects it automatically | ✅ (core test) |
| 13 | Kill switch disconnects everything and refuses reconnection until resumed | ✅ (core test + real app) |
| 14 | Removing a device removes it on both sides | ✅ (core test) |
| 15 | Permissions and roles persist and can't be changed by the peer | ✅ |
| 16 | Activity log records pairings, connections, removals; CSV export works | ✅ |
| 17 | Theme (System/Night/Day) and Motion settings apply immediately | ✅ |
| 18 | Tour visits five stops; Escape ends it; re-runnable from Help | ✅ |
| 19 | Tray: open, pair, pause/resume, quit; close-to-tray keeps devices connected | Built; needs a real desktop session to verify (see below) |
| 20 | Bluetooth step reports "no adapter" honestly when none exists | ✅ (container without an adapter) |
| 21 | Keyboard: every control reachable, visible focus ring | ✅ |
| 22 | Android: onboarding, QR scan, code entry, verification, home | ✅ compiles; logic tested on JVM |

### Not verifiable in the build environment (to check on real hardware)
These are built but could not be exercised here, which has no physical devices, GPU, Bluetooth
adapter or Android emulator:
- Android app on a phone: camera QR scanning, Keystore, foreground service, battery broadcasts.
- mDNS discovery across two physical machines on Wi-Fi (works on loopback; multicast is blocked
  in CI containers).
- Windows installer on a real Windows 11 machine (built and lint/tests pass on Windows CI).
- Tray icon behaviour on GNOME/KDE/Windows desktops (needs a session bus and a panel).
- Live Bluetooth scanning with a real adapter.
