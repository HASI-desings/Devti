# Devti for iPhone and iPad

A SwiftUI companion that uses the same Rust core as every other Devti app, through
UniFFI-generated Swift bindings. It's a **build-it-yourself** app: App Store distribution
needs a paid Apple developer account, which Devti — free and open source — doesn't require
of anyone.

## What works on iPhone

| Works | How |
|---|---|
| Pairing, verified on both screens | Scan the Hub's QR code in Devti (or with the Camera app — it opens Devti) |
| Battery on your computer | Sent while Devti is open |
| Files both ways | *Send files* picks from Files/Photos; received files are in **Files → On My iPhone → Devti** |
| Clipboard | Use the paste button to send; text copied on the computer lands on the iPhone |
| Remote for your computer | Touchpad (tap, double-tap, hold for right-click, scroll strip), keyboard, media, power |
| Shortcuts deck and commands | Designed on the Hub, run from the Deck tab |
| Your computer's screen here | Screen tab; tap to click |
| Browse shared folders | *Browse* on Home; downloads go to the Devti folder in Files |
| Find my iPhone / find my computer | Rings while Devti is open |
| Do Not Disturb everywhere, Reach from anywhere, pause | Home |

**What Apple doesn't allow** — for any app, not just Devti — and how Devti handles it:

- **Staying connected in the background.** iOS suspends apps shortly after you leave them.
  Devti connects while it's open and reconnects the moment you come back.
- **Finding devices on the network.** Browsing with mDNS needs Apple's multicast entitlement,
  granted only to App Store apps on request. The iPhone pairs by QR code (which carries the
  computer's addresses) and reconnects to the computer's last address. If that address
  changes, scan the code again — or turn on *Reach from anywhere* with Tailscale, whose
  addresses don't change.
- **Reading texts and calls, or other apps' notifications.** Not possible on iOS.
- **Sharing the iPhone's screen or being controlled.** Needs a screen-recording broadcast
  extension; Devti declines such requests and says why on the computer.
- **Reading the clipboard silently.** iOS asks every time; Devti uses the system paste
  button, which doesn't.

## Build it

You need a Mac with **Xcode 16** or later, **Rust** (`rustup`) and **XcodeGen**
(`brew install xcodegen`).

```bash
git clone https://github.com/hasi-desings/devti && cd devti
```

```bash
ios/build-core.sh
```

```bash
cd ios && xcodegen generate && open Devti.xcodeproj
```

`build-core.sh` compiles Devti Core for iPhone and the simulator into
`ios/Frameworks/DevtiCore.xcframework` and generates `ios/Generated/devti_ffi.swift`.

In Xcode, select the **Devti** target → *Signing & Capabilities* → choose your team. A free
Apple ID works: the app runs for 7 days, then rebuild it. A paid account lasts a year. Plug in
your iPhone, pick it as the destination and press **Run**. The first time, allow the developer
in *Settings → General → VPN & Device Management*.

## How it's tested

CI builds the core for the iOS simulator, generates the project and runs the unit tests on an
iPhone simulator. The tests drive the real Rust core through the Swift bindings: two nodes pair
over loopback exactly as the app does after a scan, both sides show the same code, a clipboard
and a shared-folder listing go across and decode into the app's models. Input JSON, 64-bit ids
and colour parity with Android are tested too.

## Layout

```
ios/
├── project.yml          XcodeGen spec (app + unit tests)
├── build-core.sh        Rust core → DevtiCore.xcframework + Swift bindings
├── Devti/App            entry point and scene handling
├── Devti/Core           Core (the node and app state), models, Keychain store, input
├── Devti/UI             Home, pairing + QR scanner, Remote, Deck, Screen, Browse, theme
└── DevtiTests           tests against the real core
```
