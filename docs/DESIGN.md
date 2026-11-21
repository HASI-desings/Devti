# Devti design language — "Prism"

> White light holds every color at once. The Hub is the prism; every device is one color of the
> spectrum. When a device connects, its color flows back into the prism.

This document records how the direction was chosen, the token system, the icon set and the motion
language. Every screen of the desktop Hub, and later the Android and iOS apps, follows it.

---

## 1. Three directions explored

The brief: *many devices, one living system*. It had to be stunning but never templated, with
motion that means something, and a palette that belongs to the product rather than a default.

### A. Orrery
The Hub as a sun and devices as planets on brass-ringed orbits, like a scientific instrument.

| | |
|---|---|
| Palette | Observatory navy `#121A36`, brass `#C7A25A`, vellum `#EDE6D6`, verdigris `#4F9C8C`, signal red `#D2553F` |
| Type | Fraunces (soft serif) display + IBM Plex Sans |
| Motion | Slow orbital drift; gear-precise snaps |
| Verdict | **Rejected.** Beautiful, but it looks backwards: vellum plus serif is close to the cream-and-serif look that now reads as generated. Orbits also imply a hierarchy of planets around a sun, while Devti's devices are peers. |

### B. Anodized
Devices as physical objects: bead-blasted aluminum, with each kind of device in its own anodized
color, machined toggles and magnetic snapping.

| | |
|---|---|
| Palette | Bead aluminum `#D8DADD`, graphite `#2B2E34`, cobalt `#3D5AFE`, ember `#FF6A3D`, mint `#6ED3B0` |
| Type | Schibsted Grotesk + a monospace for readouts |
| Motion | Snappy mechanical clicks |
| Verdict | **Rejected.** Excellent for controls (it informs the later shortcuts deck), but it feels like one gadget, not a living system. The mono readouts lean on a cliché. |

### C. Prism — **chosen**
Each device has a *voice*: one hue from a six-color spectrum, kept for its whole life. The Hub is
drawn as a faceted glass prism, and each facet takes the voice of a connected device. Connections
are thin fibre-optic threads that carry short pulses of light into the prism. A quiet dot field
bends around every device, like light near mass.

Why it wins:
* **Color means data.** The colors aren't decoration: a color on screen always tells you which
  device you're looking at, on the map, in lists, toggles and battery bars.
* **Living, not busy.** Movement happens where information changes (linking, light flowing,
  battery), so there is no ambient motion for its own sake.
* **Precision.** Hairline threads, crisp facets and generous negative space give the Apple-like
  restraint the brief asks for. The six voices supply the character.

**What we changed after self-review.** The first draft used large blurred aurora blobs in device
colors, which is one of the most common generated looks. We replaced them with crisp threads and
the dot field. Color now appears only where a device is.

---

## 2. Tokens (`desktop/src/styles/tokens.css`)

### Neutrals: two lights, one family
Neutrals are a cool violet ink, never pure black or a warm cream.

| Token | Night (default) | Day |
|---|---|---|
| `--bg` | `#15122A` | `#EEEDF6` |
| `--bg-deep` (map) | `#0F0C21` | `#E5E3F1` |
| `--surface` / `-2` / `-3` | `#1D1937` / `#262145` / `#312B55` | `#FBFBFE` / `#F2F1F9` / `#E8E6F4` |
| `--text` / `-dim` / `-faint` | `#F2F0FB` / `#AAA4CA` / `#736D95` | `#1A1733` / `#56517A` / `#8B87A7` |
| `--hub` (white light) | `#FFFFFF` | `#1A1733` |
| `--danger` | `#FF6680` | `#CF1F48` |

### Voices
Each voice has a luminous variant for Night and a deep variant for Day (AA contrast on each
surface). Voices are assigned from a hash of the device ID; earlier devices keep theirs and no two
devices share one until all six are used (`design/voice.ts`).

| Voice | Night | Day |
|---|---|---|
| iris | `#A596FF` | `#5643DC` |
| aqua | `#5AD6F3` | `#0B7496` |
| coral | `#FF8F7E` | `#C8432F` |
| mint | `#63E4B3` | `#0F8158` |
| marigold | `#FFCB62` | `#9C6A00` |
| rose | `#FF82C4` | `#B52A76` |

### Type
* **Bricolage Grotesque** (variable, OFL) for display. It is used *condensed* (`font-stretch: 76–88%`)
  for headings and device names, so names feel like labels on hardware.
* **Instrument Sans** (variable, OFL) for UI text: precise, neutral and readable at 13–16 px.
* Scale: major third from 14 px (12 · 13 · 14 · 16 · 20 · 25 · 31 · 39 · 49 · 61).
* Numbers use `tabular-nums`, never a monospace face.
* Sentence case everywhere, with no all-caps labels.

### Space and shape
* 4-pt spacing (4 … 72).
* Radii show hierarchy rather than one value everywhere: controls 10, panels 18, sheets 28,
  pills only for meters.

---

## 3. Icons (`desktop/src/design/icons.tsx`)

The set is hand-drawn: 24-unit grid, 1.6 stroke, round caps and joins, about 50 glyphs. No stock pack is
used.

**Signature:** every *device* icon carries exactly one filled dot, its camera or status light.
On the ecosystem map that dot lights up in the device's voice when it is online. The Hub glyph is a
prism with a white core, which is the same prism as the logo.

Distinctive choices: settings is a pair of sliders (not a gear), help is a lifebuoy, and the
ecosystem is a three-node constellation.

---

## 4. Motion language

| Moment | What moves | Why |
|---|---|---|
| **Linking** | The thread draws from the device to the prism (`pathLength 0→1`, 0.9 s), then a ring flashes in the device's voice | Shows *who* just connected |
| **Light flow** | A short dash travels along each live thread into the prism every ~5 s, offset per device | Shows the link is alive; paused when the window is hidden |
| **Unlinking** | The device springs outward to the outer ring, its thread turns into a faint dashed line and its color drains | Offline devices stay visible but quiet |
| **Parallax** | Devices float at different depths and shift slightly with the pointer; the prism shifts least | Gives the map depth; the canvas redraws only on change (no idle CPU) |
| **Prism breathing** | The white core slowly pulses | The Hub is running |
| **Welcome** | Six voices travel in from the edges and merge into the prism, lighting a facet each | The product idea in two seconds |
| **Verification code** | Digits rise in one after another | Draws attention to the one thing to compare |

Rules:
* Springs for things that move in space; ease-out curves (`--ease-out`) for fades.
* UI transitions 140–420 ms; nothing blocks input.
* **Reduced motion** (system setting or Settings → Motion → Reduced): drift, parallax, pulses and
  the welcome flight are removed, and state changes become instant crossfades. Nothing is lost
  except the movement.
* Ambient work runs only while visible: CSS pulses pause on `visibilitychange`, the dot field
  redraws on demand, and the window is destroyed when closed to the tray.

---

## 5. Accessibility
* All interactive widgets come from React Aria (keyboard, focus management, screen-reader
  semantics) with a visible 2 px focus ring in `--focus`.
* Map nodes are real buttons with full labels ("Pixel 8, connected, battery 64 percent").
* Status text never relies on color alone (every voice is paired with a name; battery has numbers).
* The verification code has a screen-reader version that reads digit by digit.
* Text contrast: body ≥ 7:1 on Night, ≥ 9:1 on Day; dim text ≥ 4.5:1.

## 6. Mobile adaptation
Android (Compose) and iOS (SwiftUI) use the same tokens (exported as Kotlin/Swift constants), the
same voices and the same prism. Their components follow platform conventions: Material 3 motion
and system sheets on Android, SF Symbols-sized hit targets and native navigation on iOS.
