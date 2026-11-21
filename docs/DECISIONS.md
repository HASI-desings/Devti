# Devti — Architecture Decision Record

Every major subsystem was compared against at least three candidates before any code was written.
Each entry lists the options, their trade-offs, license, maintenance status (checked against
crates.io / npm / upstream repos in **November 2026**) and the verdict.

Status legend: **Decided** = implemented in the current phase · **Preliminary** = chosen now so the
architecture leaves room for it, confirmed (or replaced) when that phase starts.

Project constraints that drive every verdict:

1. **$0, local-first.** No paid APIs, SDKs, servers or app stores required to build or run.
2. **Open source only,** with licenses compatible with the project license (GPL-3.0-or-later).
3. **Performance budget:** Hub idle RAM < 250 MB, idle CPU < 2 %, screen latency < 50 ms on LAN.
4. **One protocol implementation** shared by every platform, so platforms can never drift apart.
5. **Never fake a feature.** If a platform can't do something, the UI says so.

---

## D-01 · Project license — Decided

| Option | Pros | Cons |
|---|---|---|
| **GPL-3.0-or-later** | Keeps every fork open; compatible with MIT/Apache-2.0/BSD/ISC/MPL-2.0/LGPL deps; same license as KDE Connect and LibrePods, which we study | Some companies avoid GPL code |
| MIT / Apache-2.0 | Maximum adoption | Allows closed forks of a privacy tool; can't reuse ideas from GPL projects as freely |
| AGPL-3.0 | Covers network use | Devti has no server component, so the extra clause adds nothing but friction |

**Verdict: GPL-3.0-or-later.** Every dependency below has been checked as compatible. Projects we
study (KDE Connect — GPL-2+/3, scrcpy — Apache-2.0, Deskflow — GPL-2.0, LocalSend — Apache-2.0,
LibrePods — GPL-3.0, Gadgetbridge — AGPL-3.0, GalaxyBudsClient — GPL-3.0, OpenFreebuds — GPL-3.0)
are used for **protocol knowledge only**; no code is copied. Credits are listed in the README.

---

## D-02 · Desktop shell — Decided: **Tauri 2**

| Option | Pros | Cons | License | Maintenance |
|---|---|---|---|---|
| **Tauri 2** (2.12.7) | Rust backend, so the core runs in-process with no IPC bridge; uses the OS webview (WebView2 / WebKitGTK), giving ~40–90 MB idle; tray, autostart, single-instance, global-shortcut and notification plugins; MSI/NSIS/AppImage/deb/rpm bundlers built in | WebKitGTK is slower than Chromium for heavy CSS filters; the UI has to be tested on two engines | MIT/Apache-2.0 | Very active (release 2026-11-18; 3.0 in alpha) |
| Electron | One rendering engine everywhere; huge ecosystem | Ships Chromium: 150–300 MB idle RAM, which alone breaks the 250 MB budget; the Rust core would need a Node addon or a sidecar | MIT | Very active |
| Flutter Desktop | Fast Skia/Impeller rendering; could share UI with mobile | Dart ↔ Rust FFI bridge; tray and portal integrations are third-party and patchy on Linux; Wayland support still maturing | BSD-3 | Active |
| Qt 6 / QML | Native performance; mature on Linux | C++ + QML; LGPL dynamic-linking rules complicate bundling; the motion and design system would be built from scratch in QML | LGPL-3 / commercial | Active |

**Why Tauri:** it is the only option that meets the RAM budget *and* lets the Rust core, the
transport and the hardware layers (capture, input, Bluetooth) live in the same process as the UI
host. The Hub also **destroys the webview when the window is closed to the tray** and recreates it
on demand, so idle memory is just the Rust runtime (~25 MB).

---

## D-03 · Desktop UI framework — Decided: **React 19 + TypeScript + Vite**

| Option | Pros | Cons |
|---|---|---|
| **React 19 + TS + Vite 8** | Best motion library (Motion) and accessibility library (React Aria) are React-first; largest talent pool for contributors | Bigger runtime than Svelte/Solid (irrelevant on desktop) |
| Svelte 5 | Built-in transitions, tiny bundles | Smaller accessible-primitives ecosystem; Motion has no first-class Svelte API |
| SolidJS | Fine-grained reactivity, fastest updates | Smallest ecosystem; few accessible primitives |

All MIT. Vite 8 (Rolldown) gives sub-second rebuilds.

## D-04 · Accessible UI primitives — Decided: **React Aria Components**

| Option | Pros | Cons | License |
|---|---|---|---|
| **React Aria Components** (1.22) | Industry-leading keyboard and screen-reader behaviour, built-in i18n (needed for the language setting), fully unstyled | Verbose API | Apache-2.0 |
| Radix UI Primitives | Popular, unstyled | Slower release cadence since 2024; i18n left to the app | MIT |
| Base UI (1.9) | Modern successor from the Radix/MUI authors, unstyled | Younger; fewer complex widgets (no date or colour pickers yet) | MIT |
| shadcn/ui | Fast to start | It *is* a visual style; the brief explicitly rejects the default library look | MIT |

Everything is restyled from scratch with Devti tokens. No component ever ships with library styling.

## D-05 · Animation — Decided: **Motion** + CSS/WAAPI; custom SVG for the ecosystem map

| Option | Pros | Cons | License |
|---|---|---|---|
| **Motion** (14.x, formerly Framer Motion) | Layout animations, shared-element page transitions, springs, gesture-driven drag (needed for the display-arrangement editor), `useReducedMotion` | ~30 KB gzipped | MIT |
| GSAP 3 | Best timeline control | Since the Webflow acquisition GSAP is "free" but under a proprietary "Standard No Charge" license with a competing-products clause. Not OSI, and likely incompatible with GPL's no-further-restrictions rule | Proprietary-free |
| Lottie (lottie-web) | Designer-authored vector animation | Needs After Effects to author; can't react to live data (battery, signal) | MIT |
| Plain CSS + Web Animations API | Zero dependencies | No springs, no layout animation, no drag physics | — |

Every animation respects `prefers-reduced-motion` and the in-app "Reduce motion" setting.

## D-06 · Styling — Decided: **CSS Modules + design tokens as CSS custom properties**

| Option | Pros | Cons |
|---|---|---|
| **CSS Modules + tokens** | No default palette to fall back on; tokens double as the theme switch (dark/light) and are shared by name with Compose/SwiftUI | More files |
| Tailwind CSS | Fast | Its default palette and spacing are the most recognisable "template look" there is, which the brief rejects |
| vanilla-extract | Type-safe tokens | Build-plugin complexity with Vite 8 |

---

## D-07 · Sharing one core across all platforms — Decided: **Rust core + UniFFI bindings**

| Option | Pros | Cons | License |
|---|---|---|---|
| **Rust `devti-core` + UniFFI** (0.32) | One implementation of protocol, crypto, pairing and discovery for Windows, Linux, Android and iOS; generates idiomatic Kotlin and Swift with async and callback interfaces | Cross-compiling needs the NDK / Xcode toolchains | MPL-2.0 (compatible) |
| Re-implement per platform (KDE Connect style) | No FFI | Three implementations of a security-critical protocol drift apart — KDE Connect has repeatedly hit exactly this | — |
| Kotlin Multiplatform core | Shares Kotlin across Android/iOS | Desktop hub would need a JVM, which breaks the RAM budget and the Tauri integration | Apache-2.0 |
| flutter_rust_bridge | Good Rust ↔ Dart | Only useful if the mobile UI is Flutter (see D-08) | MIT |

## D-08 · Mobile apps — Decided: **Kotlin + Jetpack Compose (Android), Swift + SwiftUI (iOS)**

| Option | Pros | Cons |
|---|---|---|
| **Native (Compose / SwiftUI)** | Direct access to MediaProjection, AccessibilityService, NotificationListenerService, Telecom, SMS and Companion Device Manager on Android, and to broadcast extensions on iOS; each feels native on its platform | Two UI codebases |
| Flutter | One UI codebase | Almost every Devti feature on Android is a platform channel anyway; iOS broadcast extensions must still be native |
| React Native | Shares TS with desktop | Same platform-channel problem; heavier bridge for streaming frames |

QR scanning on Android uses **CameraX + ZXing core** (Apache-2.0). Google ML Kit and the Play
Services code scanner are rejected because they are proprietary binaries.

---

## D-09 · Transport — Decided: **QUIC via `quinn`** (control, files, media on LAN)

| Option | Pros | Cons | License / status |
|---|---|---|---|
| **QUIC (quinn 0.11)** | TLS 1.3 built in; many independent streams per connection (clipboard never waits behind a 4 GB file); unreliable datagrams for low-latency video and input; connection migration survives Wi-Fi roaming; 1-RTT reconnect | UDP can be blocked on very locked-down networks (we fall back to manual addresses and, in Phase 7, a relay) | MIT/Apache-2.0, very active |
| TLS 1.3 over TCP (tokio-rustls) | Works everywhere TCP does | Head-of-line blocking between features; we would have to build our own multiplexing | MIT/Apache-2.0 |
| WebRTC (webrtc-rs / str0m) | NAT traversal, mature media stack | Heavy signalling machinery for a LAN-first app; DTLS 1.2 identity model is awkward to pin | MIT/Apache-2.0 |
| Noise over TCP (snow) | Small, auditable handshake | Still needs our own multiplexing, flow control and congestion control | Apache/MIT |

**Stream plan:** stream 0 = control (CBOR messages, both directions); one bi-stream per file
transfer; QUIC datagrams for input events and video in Phase 3/4. ALPN = `devti/1`.

## D-10 · Authentication, pairing and encryption — Decided: **mutual TLS with pinned self-signed certs + SPAKE2 pairing + SAS**

| Option | Pros | Cons |
|---|---|---|
| **mTLS (pinned certs) + SPAKE2 PAKE + 6-digit SAS** | Every connection is TLS 1.3 with *both* sides proving key possession; pairing is secure even with a short typed code (a PAKE blocks offline brute force); the PAKE is bound to the TLS exporter and both fingerprints, so a man in the middle can't splice sessions; the SAS lets the user confirm visually | More moving parts than TOFU |
| KDE Connect style (TOFU + compare key hash) | Simple | First connection is unauthenticated; users rarely compare hashes |
| LocalSend style (HTTPS without pinning) | Simple | No device identity, so it's vulnerable to an active LAN attacker |
| Noise XX with static-key pinning | Elegant | Doesn't compose with QUIC, which mandates TLS 1.3 |

How it works (full spec in `docs/PROTOCOL.md`):

1. Each device generates an **Ed25519 key** and a self-signed X.509 cert. **Device ID** = Base32 of
   the first 10 bytes of SHA-256(SPKI); **fingerprint** = full SHA-256(SPKI).
2. The Hub opens a pairing window with a fresh secret: 128-bit in the **QR code**
   (`devti://pair?...` also carries the Hub fingerprint and LAN addresses) or a **6-digit code** for
   laptop ↔ laptop.
3. The joining device connects with QUIC. With a QR code it **pins the Hub fingerprint** from the QR.
4. Both sides run **SPAKE2** (Ed25519 group) on the secret, then exchange key-confirmation MACs over
   `(TLS exporter ‖ hub fingerprint ‖ peer fingerprint)`. Any mismatch aborts.
5. Both screens show the same **6-digit verification code** derived from the PAKE key. The user
   confirms on the Hub and the device taps "Match". Only then are fingerprints stored as trusted.
6. 3 wrong attempts burn the pairing window; windows expire after 5 minutes.
7. Later connections: the client pins the stored server fingerprint, and the server accepts only
   fingerprints in its trust store (or a pairing stream while a window is open). **No unencrypted
   fallback exists in the code.**

## D-11 · Crypto provider — Decided: **`ring`** (via rustls + quinn)

| Option | Pros | Cons |
|---|---|---|
| **ring 0.17** | Cross-compiles cleanly to Android/iOS with only a C compiler; Ed25519 support; audited | Slower release cadence (stable, maintenance mode) |
| aws-lc-rs | FIPS option; rustls default | Needs CMake + NASM on Windows and extra NDK configuration — fragile in free CI |
| RustCrypto (pure Rust) | No C at all | The rustls provider is still marked experimental |

RustCrypto crates are used for primitives outside TLS: `spake2`, `hkdf`, `hmac`, `sha2`.

## D-12 · Discovery — Decided: **mDNS/DNS-SD via `mdns-sd`** + QR addresses + manual IP

| Option | Pros | Cons |
|---|---|---|
| **mdns-sd (0.22)** | Pure Rust, no system daemon, works identically on Windows, Linux and Android; updated 2026-11-17 | Needs an Android `MulticastLock` (handled by the app) |
| zeroconf (Avahi/Bonjour wrapper) | Uses the OS responder | Requires Avahi on Linux and Bonjour on Windows (not installed by default) |
| UDP broadcast beacon (KDE Connect style) | Very simple | Broadcasts are filtered on many routers; it's a homemade protocol |
| libmdns | Responder only | Can't browse |

Service type is **`_devti._udp.local.`**, not the `_devti._tcp` from the brief: DNS-SD requires the
transport label to match the real protocol, and QUIC runs on UDP. When multicast is blocked (guest
Wi-Fi, corporate networks), the **QR code carries every Hub address**, and Settings accepts a manual
`ip:port`.

## D-13 · Wire serialization — Decided: **length-prefixed CBOR (`ciborium`)**

| Option | Pros | Cons |
|---|---|---|
| **CBOR (RFC 8949)** | Self-describing, so unknown fields and message types from newer versions are skipped rather than crashing; compact binary for file chunks; schema documented in CDDL | Slightly larger than Protobuf |
| Protobuf (prost) | Strong schema evolution | Needs `protoc` or `protox` in every build including Android NDK and iOS |
| MessagePack | Compact | Weaker standardisation of extension types |
| JSON | Debuggable | Base64 bloat for binary payloads |
| postcard / bincode | Smallest | Not self-describing, so any version skew breaks parsing |

The frame is a `u32` big-endian length followed by the CBOR envelope. The decoder enforces a
**16 MiB hard cap** and is **fuzzed** (`core/fuzz`). The local API for CLI and plugins uses JSON.

## D-14 · Local storage — Decided: **SQLite via `rusqlite` (bundled)**

| Option | Pros | Cons |
|---|---|---|
| **SQLite (rusqlite 0.43, bundled)** | Filterable activity log, battery-history time series and settings in one file; the bundled build compiles the same way on every platform | C dependency (compiled from source, no system lib needed) |
| redb | Pure Rust, ACID | Key-value only, so filtering and aggregation are hand-written |
| sled | Fast | Effectively unmaintained beta |
| JSON files | Trivial | No atomic multi-record writes; slow history queries |

## D-15 · Secret storage — Decided: **OS keychain through `keyring` 4, with a sealed-file fallback**

| Option | Pros | Cons |
|---|---|---|
| **keyring** (Windows Credential Manager, Secret Service/libsecret, macOS Keychain) | One API, MIT/Apache-2.0 | Headless or minimal Linux sessions may have no Secret Service |
| Direct per-OS calls | Full control | Three times the code |
| Encrypted file only | Always works | Key-at-rest problem |

The private key goes to the OS keychain. If no keychain is available (for example on a minimal
Linux WM without gnome-keyring or KWallet), Devti falls back to a `0600` file in the app data dir
**and says so in Settings → Security**. It never silently downgrades. On Android the key is wrapped
by an **Android Keystore** AES key; on iOS it lives in the **Keychain**. The mobile apps provide this
through a `SecretStore` callback interface into the Rust core.

## D-16 · Runtime, logging, errors — Decided

- **tokio** — required by quinn and Tauri. Alternatives (async-std, smol) are rejected because
  async-std is discontinued and smol lacks quinn integration.
- **tracing + tracing-subscriber (JSON) + tracing-appender** — structured logs with daily rotation.
  "Export diagnostics" redacts keys, secrets, tokens and file paths under the home directory. The
  alternatives were `log` + `env_logger` (no structure, no spans) and `slog` (largely abandoned).
- **thiserror** for library errors and **anyhow** at application edges.

## D-17 · Desktop battery source — Decided: **`starship-battery`**

| Option | Pros | Cons |
|---|---|---|
| **starship-battery 0.12** | Maintained fork of `battery`; Windows and Linux | — |
| battery 0.7 | Original crate | Unmaintained since 2020 |
| Read `/sys/class/power_supply` + WinAPI directly | No dependency | Duplicates a solved problem |

---

## D-18 · Testing, coverage and fuzzing — Decided

| Area | Choice | Alternatives considered |
|---|---|---|
| Rust unit/integration | `cargo test` + in-process two-node loopback tests (real QUIC, real TLS) | mocked transports (rejected: they hide real bugs) |
| Coverage | `cargo-llvm-cov` (target ≥ 80 % on core) | tarpaulin (Linux-only, ptrace flakiness), grcov (more setup) |
| Fuzzing | `cargo-fuzz` (libFuzzer, nightly) for the frame decoder, envelope parser and QR/URI parser; profile-parser target added in Phase 5 | proptest (property-based but not coverage-guided), AFL.rs (harder CI integration) |
| UI unit | Vitest 5 + Testing Library | Jest (slower, ESM pain) |
| UI end-to-end | Playwright against the built UI | Cypress (heavier; WebKit support weaker) |
| Android | JUnit + Compose UI tests; APK built in CI | — |

## D-19 · CI — Decided: **GitHub Actions** (free for public repos)

Jobs: `core` (fmt, clippy `-D warnings`, test, coverage), `fuzz-smoke` (60 s per target),
`desktop` matrix **ubuntu-24.04 + windows-2025** (lint, type-check, unit, Playwright, Tauri
build, upload bundles), `android` (cargo-ndk for 3 ABIs + Gradle `assembleRelease`, upload APK).
GitHub-hosted runners already include the Android SDK and NDK, which this project's development
sandbox can't download.

---

## Later phases — preliminary decisions

### D-20 · Screen capture (Phase 4) — Preliminary

| Platform | Choice | Alternatives |
|---|---|---|
| Windows 10/11 | **Windows.Graphics.Capture** via `windows-capture` 2.x (MIT), with Desktop Duplication (DXGI) fallback | `xcap` (CPU readback, too slow for 60 fps) |
| Linux Wayland | **xdg-desktop-portal ScreenCast + PipeWire** via `ashpd` 0.14 + `pipewire-rs` (DMA-BUF zero copy). Users see the portal prompt once; "restore token" avoids repeats | wlr-screencopy (wlroots only) |
| Linux X11 | **XShm / XComposite** via `x11rb` | `xcap` |

### D-21 · Video encode/decode (Phases 3–4) — Preliminary

| Option | Pros | Cons |
|---|---|---|
| **FFmpeg (`ffmpeg-next`) with hardware encoders** (NVENC / QSV / AMF / VAAPI), software **openh264** fallback | One API for every hardware vendor; H.264 decodes in hardware on every phone | Large DLLs on Windows; LGPL build required (no GPL-only codecs) |
| GStreamer | Pipeline flexibility | Larger runtime on Windows |
| Vendor SDKs directly | Lowest overhead | Three code paths, some with non-free licensing |

Decoding: Android **MediaCodec**, iOS **VideoToolbox**, desktop peers FFmpeg.

### D-22 · Media transport for screens (Phase 4) — Preliminary: **QUIC datagrams + per-frame streams on LAN**, WebRTC only for remote access

Same scrcpy-style design (raw H.264 NAL units with a small header) but carried over the existing
encrypted QUIC connection. Adaptive bitrate comes from quinn's RTT/loss stats.

### D-23 · Virtual / extended displays (Phase 4) — Preliminary

- **Linux:** `evdi` (GPL-2, DisplayLink's open kernel module) on X11; on Wayland, use virtual
  outputs where the compositor offers them (GNOME 46+ via Mutter's RemoteDesktop/ScreenCast virtual
  monitor; KDE via `kwin`'s virtual output). Unsupported compositors fall back to mirror/window mode.
- **Windows:** the open-source **Virtual Display Driver** (IddCx 1.10, MIT) is code-signed for free
  through SignPath Foundation, so it installs on stock Windows 10/11 x64. ARM64 on 24H2+ may need
  test-signing, so on those machines Devti defaults to mirror + window casting and explains why in
  the UI. Devti will *not* bundle or auto-install a kernel/user-mode driver silently; it guides the
  user to install it.

### D-24 · Input injection (Phase 3) — Preliminary

| Platform | Choice |
|---|---|
| Windows | `SendInput` via the `windows` crate (MIT/Apache) |
| Linux X11 | XTest via `x11rb` |
| Linux Wayland | **libei over the RemoteDesktop portal** (`reis` 0.7 + `ashpd`) — confirmed on GNOME 46+; on Plasma 6, RemoteDesktop is supported, and where libei isn't, we fall back to `uinput` (needs the user in the `input` group, explained in onboarding) |

Considered: `enigo` 0.6 (MIT). It works for X11/Windows but its Wayland path is experimental, so
Devti uses it as reference only.

### D-25 · Controlling Android from the Hub (Phase 3) — Preliminary

Two modes, both open source: **(a) Wireless companion mode** — MediaProjection for frames and
AccessibilityService `dispatchGesture` for touch, plus an Accessibility-backed IME bridge for typing.
**(b) Advanced ADB mode** — a scrcpy-style server (Apache-2.0 design, our own code) pushed over
ADB; supports full keyboard/mouse injection. Mode (a) needs no developer options; mode (b) is
faster and supports more input.

### D-26 · Bluetooth (Phase 5) — Preliminary

| Need | Choice | Alternatives |
|---|---|---|
| BLE scan + GATT, all desktops | **btleplug 0.13** (MIT/Apache) | — |
| Linux Classic profiles, RFCOMM/L2CAP, battery, A2DP/HFP control | **bluer 0.17** (official BlueZ Rust, BSD-2) over D-Bus | raw `zbus` calls |
| Windows Classic | **WinRT `Windows.Devices.Bluetooth` + RFCOMM** via `windows` crate | 32feet.NET (C#) |
| Vendor protocols | Declarative **Device Profile Library** (`profiles/*.yaml`) interpreted by the core | hard-coded per-vendor modules (don't scale) |

### D-27 · Audio relay (Phases 5–6) — Preliminary

Opus (`libopus`, BSD) over QUIC datagrams; Hub output via **PipeWire/PulseAudio** (`cpal` 0.16,
Apache-2.0) and **WASAPI** on Windows. Android playback capture uses `AudioPlaybackCapture` (Android
10+, only for apps that allow it — shown honestly in the UI).

### D-28 · Remote access outside the LAN (Phase 7) — Preliminary

| Option | Pros | Cons |
|---|---|---|
| **iroh** (QUIC-based, hole punching, self-hostable relays) | Re-uses our QUIC + pinned-key model directly | Default public relays are run by n0 (free, but third-party), so self-hosting is documented |
| WebRTC + public STUN | Ubiquitous | Separate DTLS identity, needs signalling |
| Tailscale / Headscale | Zero code: Devti just sees a LAN address | Requires the user to install another tool |

Off by default. All three are supported: Tailscale/Headscale work automatically (they look like a LAN).

### D-29 · Virtual camera / mic (Phase 6) — Preliminary

Linux: `v4l2loopback` (GPL-2) + PipeWire virtual source. Windows: a **DirectShow/Media Foundation
virtual camera** based on the OBS virtual-cam approach (GPL-2+), registered by the installer with user
consent; virtual mic through a virtual audio cable is documented, not bundled.

### D-30 · Plugin sandbox (Phase 7) — Preliminary: **WebAssembly via Extism / wasmtime**

| Option | Pros | Cons |
|---|---|---|
| **Wasm (Extism on wasmtime)** | Memory-safe sandbox; capabilities passed in explicitly; plugins in Rust/Go/JS/Zig | Plugins can't use native libraries |
| Embedded JS (QuickJS / boa) | Easy authoring | Weaker isolation guarantees |
| Out-of-process plugins over the local API | Any language | No sandbox; plugins run with user privileges |

Device profiles stay **declarative data** (YAML), so they need no sandbox at all.

---

## Deviation log

| Brief said | We did | Why |
|---|---|---|
| `_devti._tcp` | `_devti._udp` | Transport is QUIC (UDP); DNS-SD labels must match the real protocol |
| "Radix UI or shadcn/ui" | React Aria Components | Stronger a11y and i18n; shadcn is a visual style the brief rejects |
| "GSAP" as a candidate | Not used | Proprietary license with a competing-products clause, not GPL-compatible |
| Repository `HASI-desings/Devti` via `gh repo create` | Existing repo `hasi-desings/devti` (GitHub names are case-insensitive) | The repository already exists; this build session may only push to its working branch |
