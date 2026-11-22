# Extending Devti

Devti can be extended in two ways:

1. **Device profiles** — declarative YAML files that teach Devti a Bluetooth accessory's vendor
   protocol. No code runs, so they need no sandbox. *(Available now.)*
2. **Plugins** — sandboxed WebAssembly modules that add features, rule triggers and actions,
   and dashboard widgets. *(See [Plugins](#plugins) below.)*

---

## Device profiles

A profile tells Devti three things about an accessory:

- how to **recognise** it
- how to **talk** to it: the channel plus the vendor's packet framing
- what its **controls** are and how to **read** its state

From that, the Hub builds the accessory's control panel by itself. The built-in profiles live
in [`core/profiles/`](../core/profiles) and are compiled into Devti.

### Start from your device

1. Open **Accessories** in the Hub and select the device. If Devti doesn't know it yet, it uses
   the *Standard Bluetooth device* profile.
2. Click **Help support this device → Save draft**. Devti writes a draft YAML file to
   `Downloads/Devti`. The draft contains the device's name, its maker IDs and its GATT layout
   (service and characteristic UUIDs with their properties). It holds no addresses and no
   values read from the device. Nothing is ever uploaded.
3. Fill in the controls (below), test them, and open a pull request adding the file to
   `core/profiles/` and its name to `core/src/accessory/library.rs`.

### Format

```yaml
id: acme-buds            # unique, lowercase
name: Acme Buds          # shown in the UI
vendor: Acme
verified: false          # true only after testing on real hardware
notes: What works, and what doesn't yet.

match:                   # the best-scoring profile wins (maker 4, name 3, service 2)
  manufacturer_ids: [0x1234]          # Bluetooth SIG company identifiers
  names: ["Acme Buds*"]               # globs, case-insensitive
  services: ["0000fe2c-0000-1000-8000-00805f9b34fb"]

transport:               # omit for standard features only
  kind: rfcomm           # rfcomm {uuid} | l2cap {psm} | gatt {service, write, notify}
  uuid: "…"

framing: raw             # raw | sony | galaxy | huawei | nothing

init:                    # hex payloads sent after connecting
  - "01 00"

controls:
  - id: anc
    label: Noise control
    type: choice         # choice | toggle | slider | equalizer | action
    options:
      - { value: "off", label: "Off", send: "0A 00" }
      - { value: anc, label: "Noise cancelling", send: "0A 01" }
  - id: level
    label: Ambient level
    type: slider
    min: 0
    max: 10
    send: "0B {v}"       # {v} = the value as one byte (negatives as i8)
  - id: eq
    label: Equalizer
    type: equalizer
    bands: ["Bass", "Mid", "Treble"]
    min: -6
    max: 6
    offset: 6            # added before sending (here 0…12)
    send: "0C {bands}"   # {bands} = every band, one byte each

readings:                # how to read what the device sends
  - match: "0A ??"       # prefix with ?? wildcards
    kind: value
    control: anc
    byte: 1
    map: { "0": "off", "1": anc }
  - match: "10"
    kind: battery
    components:
      - { name: left, byte: 1 }
      - { name: right, byte: 2, charging_bit: 0x80 }
      - { name: case, byte: 3, charging_byte: 4 }
```

`kind: aap_battery` and `kind: aap_ear` read Apple's variable-length records.

Each profile is validated when it loads: every template and pattern must parse, ids must be
unique, and readings must point at existing controls. The unit tests in
`core/src/accessory/tests.rs` show how to check a profile against captured packets. Please add
a test with a real packet from your device.

### Framings

| Name | Shape |
|---|---|
| `raw` | Payload as is (one L2CAP packet or GATT write) |
| `sony` | `3E` type seq len(u32 BE) payload checksum `3C`, with `3C`/`3D`/`3E` escaped |
| `galaxy` | `FD` len(u16 LE) payload CRC-16/XMODEM(LE) `DD` |
| `huawei` | `5A` len(u16 BE) `00` payload CRC-16/XMODEM(BE) |
| `nothing` | `55 60 01` cmd(2) len(u16 LE) seq data CRC-16/MODBUS(LE) |

A new framing needs a small change in `core/src/accessory/codec.rs` plus a round-trip test.

### Credits

The built-in profiles reimplement protocol knowledge published by the community. Devti doesn't
include or copy their code; it reimplements what they documented:

- LibrePods (AirPods)
- Sony Headphones Client and Gadgetbridge (Sony)
- GalaxyBudsClient (Galaxy Buds)
- OpenFreebuds (Huawei)
- ear-web (Nothing)

---

## Plugins

The plugin SDK arrives with Phase 7. This section will describe its WebAssembly interface,
its permissions and a complete example plugin.
