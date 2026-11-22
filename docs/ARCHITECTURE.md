# Devti architecture

How Devti is put together, one module at a time: what each piece does, who it talks to, and
why it was built that way. For the *reasons behind library choices* see
[DECISIONS.md](DECISIONS.md); for the wire format see [PROTOCOL.md](PROTOCOL.md).

```
 ┌──────────────── Desktop Hub (Tauri) ─────────────────┐        ┌────── Android app ──────┐
 │  React UI  ──invoke/events──▶  Rust host (src-tauri) │        │  Compose UI             │
 │  (desktop/src)                 commands · tray · BLE │        │  DevtiCore (Kotlin)     │
 │                                       │             │        │        │ UniFFI + JNA   │
 │                               devti-core (Rust) ◀───┼─ QUIC ─┼──▶ devti-core (Rust)    │
 └──────────────────────────────────────────────────────┘  TLS  └─────────────────────────┘
```

One Rust core runs on every device, so the protocol, crypto and pairing logic exist exactly once.

---

## 1. Devti Core (`core/`)

### `model` — what a device is
Defines `DeviceId`, `Fingerprint`, `DeviceInfo`, `DeviceStatus`, `Permissions` and `TrustRole`.
- A **DeviceId** is derived from the device's public key (first 10 bytes of SHA-256(SPKI) in
  Base32), so it can't be forged without the private key.
- Every enum has an `Unknown`/`Other` fallback, so a newer device announcing a new kind or
  capability never breaks an older one.
- `sanitized()` cleans everything a peer sends (names, versions) before it reaches the database
  or the UI. It strips control characters and the invisible bidi/zero-width characters that could
  make one device name look like another.

### `identity` — who this device is
Generates and loads the Ed25519 key and a self-signed X.509 certificate (`rcgen`).
- The private key goes through a `SecretStore`. The certificate is public and sits in the data
  folder.
- **Recovery rules:** a lost or corrupt certificate is re-issued from the key, so the ID is kept.
  A lost key creates a new identity and the UI is told (`IdentityOrigin::Regenerated`) because old
  pairings become invalid.
- The certificate's dates are irrelevant: trust comes from pinning, not from a CA.

### `secret` — where the private key lives
A `SecretStore` trait with four implementations: OS keychain (`keyring`, feature `keychain`), a
permission-locked file (0700 directory, 0600 file), in-memory (tests), and on mobile a
Kotlin/Swift implementation passed over UniFFI. `best_available()` tries the keychain with a
write/read/delete probe and only then falls back, and the backend's name is shown in
Settings → Security so a downgrade is never silent.

### `protocol` — the wire format
- `frame`: `u32` big-endian length plus CBOR body. The length is checked against a per-stream limit
  *before* allocating (16 KiB on pairing streams, 1 MiB on control, 16 MiB hard cap), and CBOR
  nesting is capped at 64.
- `messages`: `StreamOpen` (first frame of every stream), `PairMsg`, `ControlMsg`, abort/goodbye
  reasons, QUIC close codes. Unknown messages decode to `Unknown` and are skipped.
- Fuzzed by `core/fuzz` (three targets).

### `pairing` — the cryptography of trust
- `uri`: the QR link `devti://pair?...` (hub ID, fingerprint, 128-bit secret, addresses, port,
  name). Parsing is strict and capped at 1 KiB; it is fuzzed because QR codes come from anyone.
- `pake`: SPAKE2 (Ed25519 group) → shared key K → HKDF keys salted with a **transcript** of the
  TLS exporter, both fingerprints and both SPAKE messages → HMAC key confirmation plus a 6-digit
  SAS. Different TLS sessions, which is what a man in the middle produces, mean different
  transcripts, so the MACs can never verify on both legs.
- Secrets are domain-separated (`qr:` / `code:`), so a QR secret can never be used as a typed code.

### `tls` — admission control
rustls TLS 1.3 configs with custom verifiers:
- **Client side:** the server's SPKI fingerprint must equal the pinned one (from the trust store,
  or from the QR). The only exception is code pairing, where the PAKE authenticates instead.
- **Server side:** a `ClientPolicy` admits a client certificate only if its fingerprint is trusted,
  or a pairing window is open, and never while paused.
- In every case rustls still checks the handshake signature, so the peer proves it holds the key.

### `discovery` — finding devices
mDNS/DNS-SD with `mdns-sd`. The service is `_devti._udp.local.`, the instance name is the device ID,
and the TXT record carries the name, kind, platform, fingerprint and a pairing-open flag. Results
are *hints only*: loopback and link-local addresses are dropped, and a fingerprint that doesn't
match the ID is discarded. Identity is decided by TLS, never by mDNS.

### `store` — memory
One SQLite file (`rusqlite`, bundled) with numbered migrations on `PRAGMA user_version`. It holds
trusted peers, settings and the activity log. A database from a *newer* Devti is refused rather
than corrupted, and an unreadable peer row is skipped and logged, never fatal.

### `node` — the running ecosystem
`Node` is the public API every app uses. Internally:
- **`trust`**: the live admission view (trusted fingerprints, window open, paused) shared with the
  TLS verifier.
- **`session`**: one QUIC endpoint both accepts and dials. `serve_connection` reads each stream's
  `StreamOpen` header and routes it. A **session** is an authenticated control stream with
  Hello → status/info/ping/goodbye. If both devices dial each other at once, both keep the
  connection started by the lower device ID, so they converge without coordination.
  `reconnect_loop` redials offline peers with 1→30 s exponential backoff (±20 % jitter), trying
  fresh mDNS addresses first and the last known address second.
- **`ceremony`**: the network choreography of pairing (diagram in PROTOCOL.md). The hub reserves
  the window (one pairing at a time), three wrong secrets burn it, and the joiner opens the control
  stream only after the hub's `Done`, which removes a race between "trusted" and "first message".
- **Threading:** all tasks are spawned on the runtime handle captured at `start`, so UI threads
  (Tauri sync commands, Android main thread) can call any method safely. This came from a real bug
  found by driving the actual app; a regression test now covers it.
- **Events:** a broadcast channel of `NodeEvent` (JSON-tagged). The desktop and mobile UIs both
  render from it.
- **`continuity`** (Phase 2): clipboard (history of 50, echo suppression), find my device,
  notifications (the owner keeps the truth and re-sends on reconnect; receivers keep a mirror
  per device and clear it when the device disconnects), ecosystem-wide Do Not Disturb, battery
  history (30 days, de-duplicated within 10 minutes) and one-shot low-battery alerts. Each
  incoming request is checked against the permissions this device granted the sender.
- **`transfer`** (Phase 2): one QUIC stream per file. Offer → answer (with a resume offset) →
  raw bytes → SHA-256 → saved. Partial files survive drops; when the device comes back the
  sender resumes automatically. A small retry scheduler fixes a race in which the device
  reconnected before the interrupted attempt had finished unwinding. Received names are made
  safe and never overwrite existing files.
- **`control`** (Phase 3): checks the local permission for every remote request, writes the
  activity log, and turns each request into an event for the host (`RemoteInput`,
  `SystemRequested`, `LaunchRequested`, `RunRequested`, `DeckPressed`). The core never touches
  the OS itself. It keeps the **catalog** the host publishes (apps, commands, deck, system
  actions) and answers catalog requests, trimmed to what the asker may use.
- **`screen`** (Phases 3–4): live screens as JPEG frames on a `screen` stream. The sharer keeps
  only the latest frame (a `watch` channel) and at most two in flight, measures latency from
  acks, and adapts the target quality and width the host encoder reads (`screen_target`).
  Viewers receive frames on their own broadcast channel (`subscribe_frames`), kept separate from
  the JSON event bus.
- **`api`** (feature `api`, Hub only): the local HTTP + WebSocket API (axum on 127.0.0.1, bearer
  token, loopback-Host check). The `cli/` crate is its client: `devti send|ring|clip|run|watch`.

---

## 2. Mobile bridge (`core/ffi/`)

UniFFI exposes one object, `DevtiMobile`, with two callback interfaces:
`EventListener` (events out, as JSON) and `PlatformSecretStore` (Keystore/Keychain in). Rich data
crosses as the same JSON the desktop uses, which keeps the binding surface small and stops the
two UIs from drifting apart. The node runs on its own 2-thread Tokio runtime inside the library.
`uniffi-bindgen` (feature `bindgen`) generates the Kotlin, and later the Swift, sources.

---

## 3. Desktop Hub (`desktop/`)

### Rust host (`src-tauri/`)
- **`lib.rs`** boots in this order: logging (JSON file plus stderr, daily rotation, 7 files),
  then the node with keychain-backed secrets, then the event forwarder (`devti://event`, with a
  `resync` signal if the UI fell behind), battery polling, the tray, and finally the window.
  With `--hidden` (autostart) no window is created at all.
- **Close to tray:** closing the window *destroys* it; `ExitRequested` is intercepted so the
  process (and every connection) stays alive. The tray recreates the window from the config. This
  is how idle memory stays far below the 250 MB budget.
- **`commands.rs`**: typed wrappers around `Node`. Settings writes are allow-listed, so the UI
  can't touch internal keys.
- **`tray.rs`**: open, pair, pause/resume (text kept in sync with node events), quit. A "pair"
  request made before the window loads is queued and picked up by `take_ui_action`.
- **`bluetooth.rs`**: a 90-second BLE scan with `btleplug`. It labels standard services and
  manufacturer IDs, throttles updates, and reports "no adapter" or "Bluetooth off" as status
  messages instead of errors.
- **`system.rs`**: battery (`starship-battery`), OS version, Wayland/X11 detection, and the
  diagnostics export, which redacts pairing secrets, codes and home paths.
- **`continuity.rs`** (Phase 2): polls the OS clipboard once a second with `arboard` (no
  portable change event exists) and writes received text back. It posts native notifications
  for mirrored phone alerts (silent during DND), low battery and received files, and parses
  `--send <paths>` for "Send to Devti" (forwarded by the single-instance plugin).
- **Files commands** open or reveal only paths inside the received-files folder, so the UI
  can't be used to open arbitrary files.
- **`remote.rs`** (Phase 3) carries out what remotes ask:
  - Input: an `enigo` injector on its own thread (SendInput, XTest, libei/virtual input on
    Wayland).
  - Power, volume and brightness through each OS's own tools, with fallbacks.
  - Installed apps (`.desktop` files or Start-menu shortcuts).
  - Saved commands (`sh -c` / `cmd /C`, 60-second limit, output returned).
  - Deck actions: hotkeys, media keys, text, apps, commands, links and OBS scenes (a minimal
    OBS WebSocket v5 client).
  - The local API, which starts with the Hub and can be turned off.

  Live frames reach the web view through a Tauri `Channel` as raw bytes (12-byte header +
  JPEG), never as JSON.
- **`screens.rs`** (Phase 4) shares this computer's screen with devices that ask to see it:
  - One capture thread runs: xcap → `fast_image_resize` to the core's adaptive width →
    `jpeg-encoder` at its quality → `push_screen_frame`, paced to the target fps.
  - It starts with the first viewer and stops with the last. A sampled hash skips frames that
    haven't changed.
  - On Wayland, frames come from the ScreenCast portal (PipeWire) after the system dialog.
- **`edges.rs`** (Phase 4), one keyboard and mouse:
  - A platform-free core handles edge hits, entry and return points, the return condition and
    key translation.
  - The X11 backend polls the pointer, then grabs the pointer and keyboard, parks the real
    pointer in the middle and forwards relative motion, buttons, wheel and keys.
  - The Windows backend uses low-level hooks.
  - Either way, Scroll Lock or moving back past the edge returns control.

### React UI (`src/`)
- **`bridge/`**: one `Bridge` interface with two implementations. `tauri.ts` (the real one) is
  `invoke` plus `listen`. `preview.ts` is a clearly labelled simulation used only in a plain
  browser for design review and UI tests; the app shows a permanent *Preview* badge in that mode.
- **`state/store.ts`** (zustand): folds `NodeEvent`s into UI state, assigns voices, and tracks
  "just linked" devices for the link animation. It is unit-tested.
- **`design/`**: the custom icon set, the `Prism` hub glyph and voice assignment (see DESIGN.md).
- **`features/`**: onboarding (7 steps), ecosystem map (SVG threads, HTML nodes and a canvas dot
  field that only redraws on change), pairing panel, device page, security, settings, help, tour.
  Phase 2 adds **files** (drop anywhere or on a device, live progress, clipboard history),
  **notifications** (grouped by device, inline quick reply, DND), **battery** (one chart per
  device, palette validated for colour blindness, table view) and the full-screen **ring**
  overlay with a WebAudio chime. Phase 3 adds:
  - **control**: the deck editor with an action inspector, saved commands with test runs, and the
    local API panel.
  - **device control**: media keys, power with confirmation, the device's commands and apps.
  - **mirror**: a canvas fed by `createImageBitmap`. Click, hold, drag, wheel and right-click map
    to tap, long press, swipe, scroll and back; keys type; screenshots and MediaRecorder
    recordings are saved through `save_capture`. For a computer, the mirror view acts as a remote
    desktop: absolute pointer, buttons, wheel and shortcuts.
  - **screens**: what to show (monitors and windows), who is watching with live stats, and the
    edge arrangement for one keyboard and mouse.
- **`ui/`**: React Aria primitives restyled with Devti tokens.

---

## 4. Android app (`android/`)

- **`core/state/`** (platform-free Kotlin): models and a pure `Reducer` (state + event JSON →
  state). `android/jvm-check` compiles it on a plain JVM together with the generated bindings and
  pairs two real Rust nodes through JNA, so the integration is tested without an emulator.
- **`DevtiCore`**: owns the `DevtiMobile` node. It feeds battery broadcasts and network-change
  callbacks into the core, holds a Wi-Fi `MulticastLock` so mDNS works, and exposes a `StateFlow`.
- **`KeystoreSecretStore`**: an AES-256-GCM key that never leaves the Android Keystore seals the
  device key. The data is excluded from cloud backup and device transfer.
- **`DevtiService`**: a `connectedDevice` foreground service with a live, honest notification
  ("Connected to Studio Laptop").
- **UI**: Compose screens in the same Prism language. The CameraX + ZXing scanner works without
  Google Play Services, and `devti://pair` deep links mean the system camera can start pairing too.
- **Build**: Gradle runs `cargo ndk` for arm64-v8a, armeabi-v7a and x86_64, and `uniffi-bindgen`
  for the Kotlin sources, before `preBuild`.
- **`continuity/`** (Phase 2), the Android side of each feature:
  - `NotificationMirror`, a `NotificationListenerService`, filters noise (Devti's own
    notifications, ongoing ones, group summaries and foreground services) and converts the rest:
    big text, up to three actions with reply flags, and a 64 px icon. It runs actions with
    `RemoteInput` exactly as if typed on the phone, dismisses, and keeps the phone's DND and
    the ecosystem's in step, without overriding the user's own DND type.
  - `ClipboardBridge` writes any time. It reads only while Devti has focus, because Android 10+
    forbids background reads: on focus, while open, via "Send clipboard", or with text shared
    to Devti.
  - `Ringer` plays the alarm sound at alarm volume, restoring the volume afterwards, vibrates,
    and shows a full-screen `RingActivity` over the lock screen. It stops itself after three
    minutes.
  - `Files` copies shared `content://` documents into the cache before sending, because those
    URIs don't stay readable long enough for resumes. Received files move to
    **Downloads/Devti**: MediaStore on Android 10+, or the public folder on 8–9 when storage
    permission was granted, otherwise they stay in app storage behind a `FileProvider`.
  - `ShareActivity` adds "Send with Devti" to the share sheet: files go to the chosen device,
    and text goes to the clipboard.
  - `Pure` holds platform-free decisions (notification filter, safe names, sizes), unit-tested
    on the JVM.
- **`control/`** (Phase 3):
  - `ControlAccessibility` ("Devti control", which the user turns on) performs gestures
    (`dispatchGesture`), typing into the focused field (`ACTION_SET_TEXT`) and global actions.
  - `ScreenShareActivity` asks for MediaProjection consent. `ScreenCaptureService` (a
    `mediaProjection` foreground service) mirrors the display into an `ImageReader` at the
    core's target width, then compresses JPEGs at the target quality and rate.
  - `PhoneCommands` builds the phone's catalog (launchable apps, flashlight, screenshot, camera)
    and runs it.
- **Screen tab and `ScreenViewerActivity`** (Phase 4): a computer's screen shown full screen.
  Frames come through the `FrameListener` callback into a conflated `StateFlow` and are decoded
  off the main thread. Touch maps to mouse: tap is a click, hold is a right-click, one finger
  drags, two fingers scroll, and a pinch sends Ctrl +/−.
- **UI tabs**: Home, **Remote**, **Screen** and **Deck**.
  - The remote's touchpad turns raw pointer events into accelerated, batched moves (12 ms),
    clicks, right-clicks, scrolls and drags.
  - Its keyboard is an invisible text field that diffs what the IME typed into text and
    backspace events, so any keyboard app and language works.
  - In the presenter, the volume keys turn slides. `core/state/Input.kt` builds the
    `InputEvent` JSON and is unit-tested.

---

## 5. Testing strategy

| Layer | How | Where |
|---|---|---|
| Core logic | Unit tests per module | `core/src/**` |
| Core end-to-end | Real nodes over loopback QUIC: QR and code pairing, wrong-code lockout, MITM pin refusal, revoke, reconnect after restart, kill switch, off-runtime calls | `core/tests/node.rs` |
| Parsers | libFuzzer (frames, messages, QR links) | `core/fuzz` |
| Mobile bridge | Kotlin bindings + reducer against real Rust nodes on the JVM | `android/jvm-check` |
| UI logic | Vitest (store, voices, formatting, icons) | `desktop/src/**/*.test.ts` |
| UI flows | Playwright on the built UI (preview mode) | `desktop/tests/e2e` |
| Real app | WebDriver drives the actual WebKitGTK binary to pair with `sim_device` over real QUIC | `desktop/tests/real/smoke.mjs` |

`core/examples/sim_device.rs` is a headless simulated phone for development: it pairs via link
or code, auto-confirms, and reports a draining battery.
