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

## Phase 3 — final decisions

### D-31 · Screen frames (Phases 3–4) — Decided: **adaptive JPEG frames on a QUIC stream**, H.264 later

| Option | Pros | Cons |
|---|---|---|
| **JPEG per frame** (Android `Bitmap.compress`, desktop encoder later) | Decodes everywhere with `createImageBitmap`, including WebKitGTK; no FFmpeg DLLs; no codec licensing; every frame stands alone, so dropping one costs nothing | 3–10× the bandwidth of H.264 (about 2–8 Mbit/s at 720p, fine on a home network) |
| H.264 via MediaCodec → WebCodecs | Hardware encode on the phone, low bandwidth | WebKitGTK's WebCodecs H.264 support varies by distro, so Linux needs a software decoder anyway; keyframe and loss handling to build |
| H.264 → openh264 in Rust → raw RGBA to the web view | Works on every OS | ~110 MB/s of pixels through IPC at 720p 30 fps |

**Flow control** is the part that matters for latency. The sharer sends only the *newest* frame
and never has more than two unacknowledged ones in flight. A slow network therefore lowers the
frame rate instead of building a queue. Capture-to-ack latency drives a quality ladder (JPEG
quality first, then width) that the encoder follows. The frame message is typed (`ScreenMsg`), so
an `h264` variant can be added later without breaking older peers.

### D-32 · Input injection (Phase 3) — Decided: **`enigo` 0.6** (revises D-24)

0.6 now ships a libei backend and Wayland virtual-keyboard/pointer protocols next to XTest (X11)
and SendInput (Windows). Writing three backends ourselves would add risk and no capability, so
Devti adopts it, on a dedicated thread. On Wayland, GNOME asks once for remote-desktop
permission. Compositors with neither libei nor the virtual-input protocols can't be controlled,
and the Hub says so in a notice instead of failing silently.

### D-33 · Controlling Android (Phase 3) — Decided: **wireless companion mode** (D-25a); ADB mode deferred

MediaProjection captures the frames. The user confirms every session, because Android allows no
"remember". An accessibility service ("Devti control", which the user turns on) performs the
gestures. Typing uses `ACTION_SET_TEXT` on the focused field. The service subscribes to no
events and stores nothing. The scrcpy-style ADB mode (D-25b) is deferred: it needs developer
options and a USB or `adb tcpip` setup that most users won't do. The wireless mode covers mirror,
tap, swipe, type, navigation, screenshots and recordings.

### D-34 · Local API (Phase 3) — Decided: **axum 0.8 on 127.0.0.1 with a bearer token**

| Option | Pros | Cons |
|---|---|---|
| **HTTP + WebSocket on loopback (axum)** | `curl`-friendly on every OS; WebSocket for events | Must defend against browsers: Host-header check (DNS rebinding), no CORS, Origin refused on the socket |
| Unix socket / named pipe | OS file permissions do the auth | Two transports; awkward from scripts on Windows |
| gRPC | Typed clients | Heavy for a handful of calls |

The token is 32 random bytes in `<data dir>/api-token`, mode 0600 on Unix. It can be reset from
Settings. The `devti` CLI is a separate small crate (`ureq` + `tungstenite`, both blocking) that
reads the same files.

### D-35 · OBS scene switching — Decided: **own minimal OBS WebSocket v5 client**

The protocol needs three messages: Hello, Identify with SHA-256 challenge auth, and one Request.
The `obws` crate would add a large dependency tree for that. Our implementation is checked
against the worked example in the obs-websocket spec, and only `ws://127.0.0.1` / `localhost`
URLs are accepted.

## Phase 4 — final decisions

### D-36 · Desktop capture — Decided: **xcap 0.9** (revises D-20)

xcap covers the three sources Devti needs behind one API: X11 (XShm/XGetImage), Windows
(DXGI/GDI) and Wayland (xdg-desktop-portal ScreenCast + PipeWire, or wlr-screencopy). It
captures monitors and windows, and Wayland uses the portal, so the user always picks the screen
or window in the system dialog. `windows-capture` and a hand-written PipeWire client would cover
less for more code. Frames are scaled with `fast_image_resize` (SIMD) and encoded with
`jpeg-encoder`, matching D-31's JPEG transport. A sampled hash skips unchanged frames, so a still
desktop costs almost nothing.

### D-37 · Extended displays — Decided: **use a virtual monitor when one exists** (D-23 confirmed)

Devti doesn't install display drivers. When a virtual monitor exists (the signed Virtual
Display Driver on Windows, a dummy plug, or `evdi` on Linux), it appears as one more source on
the Screens page. Showing it on a device turns that device into an extended display. Without a
virtual monitor, Devti offers mirror and single-window modes, and the page explains this.

### D-38 · One keyboard and mouse — Decided: **own edge switching on X11 and Windows**

| Option | Pros | Cons |
|---|---|---|
| **Built into the Hub** | One app; it uses the existing encrypted session and per-device permissions | We write the input capture per platform |
| Bundle Deskflow | Mature | A second daemon with its own pairing and TLS, which duplicates Devti's trust model |
| InputCapture portal (libei) | The proper Wayland answer | Only on very recent GNOME; no stable Rust client yet |

The X11 version polls the pointer and grabs pointer and keyboard while the other computer has
control, parking the real pointer in the middle. The Windows version uses low-level hooks.
Geometry, the return condition and key translation are platform-free and unit-tested. Wayland
is listed as unsupported, with the reason.

## Phase 5 — final decisions

### D-39 · Accessory protocols — Decided: **declarative YAML profiles + a small set of framings in core**

| Option | Pros | Cons |
|---|---|---|
| **YAML profiles + built-in framings** | Contributors add devices without writing code; profiles need no sandbox; the UI is generated | Unusual protocols need a new framing in Rust |
| Code per device (like most clients) | Anything is possible | Every device is a code review; nothing generated |
| Plugins (Wasm) per device | Sandboxed code | Heavier for what is mostly byte templates |

Profiles hold byte templates (`{v}` / `{bands}` placeholders), wildcard match patterns, and
battery/value readers. Framers handle Sony, Galaxy, Huawei and Nothing packets. They
resynchronise after corruption and are fuzzed.

### D-40 · Classic Bluetooth channels — Decided: **BlueZ via `bluer` on Linux; BLE via `btleplug` elsewhere** (D-26 refined)

`bluer` is BlueZ's official Rust binding. It gives L2CAP sockets, GATT, device properties
including `Battery1`, and RFCOMM through BlueZ's Profile API, so Devti never needs the SDP
channel number. Windows has no free, stable Rust binding for RFCOMM/L2CAP yet (WinRT RFCOMM
needs app-container capabilities). So on Windows the Hub shows BLE accessories and standard
batteries, and it says that Classic vendor controls need a Linux Hub for now.

### D-41 · Honesty about untested protocols

The built-in profiles reimplement community documentation. None has been run against real
hardware in this project yet, so they are `verified: false` and the UI says **"Untested on real
hardware"**. JBL's party mode and EQ use a closed protocol nobody has documented, so the JBL
profile only names the device and says why.

## Phase 6 — final decisions

### D-42 · Texts and calls — Decided: **the phone's own SMS store and telephony APIs**

| Option | Pros | Cons |
|---|---|---|
| **Read Android's SMS provider, send with `SmsManager`, watch `PHONE_STATE`, act with `TelecomManager`** | Works next to the user's messaging app; texts go from their number | Needs SMS, contacts and phone permissions (the app asks, and says why); no MMS or RCS |
| Become the default SMS app | Full control, MMS | Replaces the user's messaging app — far too invasive |
| Pair with Google Messages for Web / RCS | Rich chats | Closed, undocumented protocol; account-bound |

Call **audio** stays on the phone: Android only lets the default dialer (`InCallService`) or VoIP
apps (`ConnectionService`) touch a carrier call's audio. Devti shows the call card and its
buttons (answer, decline, end, mute); to talk through the computer, the user also pairs the
phone with it as a Bluetooth headset (Hands-Free Profile, handled by the OS). Said in the app,
the Hub and the README.

### D-43 · Live audio — Decided: **raw 16-bit PCM over a QUIC stream** (revises D-27)

| Option | Pros | Cons |
|---|---|---|
| **PCM s16le, 48 kHz, 20 ms chunks** | No codec to build for three platforms; zero codec delay; easy to verify | ~1.5 Mbit/s stereo — fine on a LAN, too much over the internet |
| Opus (`libopus`) | ~10× smaller | C library cross-compiled for Android ABIs and Windows; adds a codec to fuzz |
| WebRTC audio | Echo cancellation, FEC | Heavy dependency, its own transport and identity |

Devti is LAN-first, so PCM wins for now; Opus stays the upgrade path for Phase 7's remote
access. The Hub plays through `pacat` (PipeWire/PulseAudio) on Linux and `cpal` (WASAPI) on
Windows. The phone-as-microphone on Linux is a PipeWire/PulseAudio null sink plus a
remapped source named "Devti-phone-microphone", which any app can choose.

### D-44 · Phone camera as a webcam — Decided: **reuse the screen frame path; v4l2loopback on Linux** (refines D-29)

The phone sends CameraX frames through the same adaptive JPEG stream as screen sharing
(`ScreenStart.source = "camera"`), so latency handling and tests are shared. On Linux the Hub
writes them to a `v4l2loopback` device as YUYV, so browsers and video apps see a camera.
Windows has no free virtual-camera driver Devti can ship unsigned, so the camera shows in the
Hub window and the app explains how to pass it on with OBS's free virtual camera.

### D-45 · Automation — Decided: **a small declarative rules engine in core**

| Option | Pros | Cons |
|---|---|---|
| **Triggers + actions as JSON rules, edge-triggered, with cooldowns** | Easy to edit in a form, to test and to explain; can't run arbitrary code | Only what we built in |
| Embedded scripting (Rhai / Lua) | Unlimited | A sandbox to secure, and a language to learn |
| Home Assistant / Node-RED integration | Huge ecosystem | Requires running another server |

Arbitrary code is Phase 7's job (Wasm plugins with permissions). Rules fire on the *edge*
(battery crossing below a level, not every report below it) and wait out a cooldown.

### D-46 · Proximity — Decided: **the phone advertises a rotating token; the Hub listens**

| Option | Pros | Cons |
|---|---|---|
| **BLE service data with an 8-byte HMAC token that changes every 15 minutes** | Low power; nobody else can recognise or follow the phone; no connection | Signal strength is noisy (smoothed, with hysteresis) |
| Wi-Fi presence (connection drops) | No Bluetooth | Takes tens of seconds; phones sleep their Wi-Fi |
| RSSI of a connected Bluetooth link | Accurate | Needs a permanent connection |

The token is HMAC-SHA256(the phone's certificate fingerprint, "devti-proximity" ‖ 15-minute
epoch), first 8 bytes. Only paired computers know the fingerprint; they accept the current and
the previous epoch. Near ≥ −65 dBm, away ≤ −80 dBm (smoothed), and "away" after 30 s silent.
The Hub scans without a service filter because the UUID only travels as service data.

### D-47 · Handoff — Decided: **links (with media time) and text, nothing else**

A handoff opens a web link (http/https only — checked on both ends) or puts text on the
clipboard. YouTube links carry `t=` so a video resumes at the same second. On Linux, "continue on
phone" reads the playing media's URL and position over MPRIS; elsewhere it sends the link on the
clipboard.

## Phase 7 — final decisions

### D-48 · Reaching devices away from home — Decided: **exchange reach addresses; private networks do the routing** (D-28 refined)

| Option | Pros | Cons |
|---|---|---|
| **Each device tells its peers its overlay-network and typed addresses; peers dial them with the same pinned TLS** | No server of ours; works with Tailscale/Headscale, ZeroTier, WireGuard and port forwarding; zero new trust | The user needs one of those (Tailscale is free and two taps) |
| iroh relays + hole punching | Works with no setup | Default relays are a third party's; self-hosting a relay is a server to run |
| WebRTC + public STUN/TURN | Ubiquitous | A second identity system and a signalling server |

The preliminary note said Tailscale "looks like a LAN" — it doesn't for discovery (mDNS
doesn't cross it), so devices now *tell* each other their reach addresses once they meet.
Peers keep them (≤ 16, validated), resolve DNS names at each dial, and try them after local
ones. Any address can be wrong or hostile without harm: the connection still has to present
the paired certificate. Off by default; the UI shows which path a session uses.

### D-49 · Plugins — Decided: **Extism on wasmtime, permissioned host functions** (D-30 confirmed)

Extism was the preliminary pick and held up: it gives memory and time limits, an HTTP host
allow-list and PDKs for many languages. Devti adds the policy: WASI off (no files), 64 MiB,
2 s per call, one thread per plugin, host functions that check a plain-language permission
list the person approved, re-approval when an update asks for more, and events only for the
categories a plugin asked for. Commands join the existing catalog, so phones and rules use them
with no new protocol. Cost: the Hub carries the wasmtime runtime, which makes the installers larger (measured in QA.md).

### D-50 · Shared folders — Decided: **virtual paths, resolved and re-checked on the owner**

| Option | Pros | Cons |
|---|---|---|
| **Owner shares named folders; peers see `/<name>/…`; owner canonicalises and re-checks every request** | No real path ever leaves the device; symlink and `..` escapes fail closed; downloads reuse resumable transfers | Read-only |
| SMB / SFTP servers | Full file systems | Accounts, ports and services to secure — not $0 to operate safely |
| Sync engine (Syncthing-like) | Offline copies | A different product |

Browsing needs a new `browse` permission, on for trusted devices; records saved before it
existed read as off. On Android, sharing the phone's folders needs "All files access"
(Android 11+), asked only when the person turns sharing on.

## Phase 8 — final decisions

### D-51 · iPhone and iPad — Decided: **SwiftUI over the same Rust core (UniFFI), built by each person**

| Option | Pros | Cons |
|---|---|---|
| **Native SwiftUI + UniFFI Swift bindings to the shared core** | One protocol and crypto implementation everywhere; native feel; small | Needs a Mac to build |
| A Tauri 2 mobile build of the Hub UI | Reuses the React UI | A desktop-shaped UI on a phone; the same Mac requirement |
| A web app (PWA) | No install | Can't do QUIC, local network or Keychain — not Devti |

App Store distribution needs a paid developer account; Devti asks nobody to pay, so the iPhone
app is build-it-yourself (a free Apple ID signs it for 7 days). Apple's limits are stated in
the app and in ios/README.md. Discovery is off on iOS: mDNS needs Apple's multicast
entitlement, which only App Store apps can request. Pairing therefore uses the QR code
(which carries the addresses), reconnection uses the last address and reach addresses
(D-48). CI builds the core for the simulator and runs the Swift tests against it.

### D-52 · Proving the performance targets — Decided: **measure the release build in CI, every run**

The brief sets idle CPU < 2 %, Hub RAM < 250 MB and screen latency < 50 ms. A script drives
the release-built Hub under a virtual display with a simulated phone over real QUIC: the phone
watches the real screen (1280 px, 30 fps, a constantly changing clock on screen) and reports
capture-to-acknowledged latency; then, connected but idle, the Hub's CPU is sampled for 60 s
and its memory summed as PSS across the app and its WebKit processes. Frames are stamped
when the screen is captured, so capture and JPEG encoding count towards latency. CI fails
when a budget is exceeded, and uploads the numbers (QA.md records them).

### D-53 · Releases — Decided: **tag-triggered GitHub Actions builds, checksums, optional signing key**

Every installer is built by CI from the tag, never on a laptop; `SHA256SUMS.txt` sits next to
the files. Android uses a stable release key when the repository has one (docs/RELEASING.md);
otherwise the release notes say it's a debug-signed build. Windows and Linux builds are
unsigned — code-signing certificates cost money — and the README says how to proceed.

---

## Deviation log

| Brief said | We did | Why |
|---|---|---|
| `_devti._tcp` | `_devti._udp` | Transport is QUIC (UDP); DNS-SD labels must match the real protocol |
| "Radix UI or shadcn/ui" | React Aria Components | Stronger a11y and i18n; shadcn is a visual style the brief rejects |
| "GSAP" as a candidate | Not used | Proprietary license with a competing-products clause, not GPL-compatible |
| Repository `HASI-desings/Devti` via `gh repo create` | Existing repo `hasi-desings/devti` (GitHub names are case-insensitive) | The repository already exists; this build session may only push to its working branch |
| D-21/D-22: H.264 for screens | Adaptive JPEG frames (D-31) | Decodes in every web view without FFmpeg; latest-frame flow control keeps latency low; H.264 stays an upgrade path |
| D-24: own input backends | `enigo` 0.6 (D-32) | It now covers libei/Wayland, XTest and SendInput |
| D-25b: ADB mode for Android control | Deferred (D-33) | Needs developer options; the wireless mode covers the features |
| D-20: windows-capture + PipeWire by hand | xcap 0.9 (D-36) | One API for X11, Windows and Wayland (portal) |
| "One keyboard and mouse" on every desktop | X11 and Windows; not Wayland (D-38) | Wayland has no stable input-capture API yet |
| Vendor controls on Windows and Linux | Linux (BlueZ) now; Windows BLE + standard battery (D-40) | No free, stable RFCOMM/L2CAP binding on Windows yet |
| D-27: Opus audio over datagrams | PCM over a QUIC stream (D-43) | No codec to cross-compile; LAN bandwidth is plentiful; Opus kept for remote access |
| D-29: Windows virtual camera | Camera shown in the Hub; OBS virtual camera documented (D-44) | A camera driver must be signed to install; no free signed one Devti can ship |
| Calls on the computer | Call card and controls; audio via Bluetooth Hands-Free (D-42) | Android gives carrier-call audio only to the default dialer |
| D-28: "Tailscale looks like a LAN" | Devices exchange reach addresses (D-48) | mDNS doesn't cross overlay networks, so peers must be told the addresses |
| iOS app in the App Store | Build-it-yourself (D-51) | App Store distribution needs a paid account; Devti costs nothing |
| mDNS discovery on every platform | Not on iOS (D-51) | Apple restricts multicast to apps granted its entitlement |
