# Extending Devti

Devti can be extended in two ways:

1. **Device profiles** — declarative YAML files that teach Devti a Bluetooth accessory's vendor
   protocol. No code runs, so they need no sandbox. *(Available now.)*
2. **Plugins** — small WebAssembly programs that add commands (on the Hub, your phone's
   remote and deck, and in rules) and react to events, inside a sealed sandbox.
   *(See [Plugins](#plugins) below.)*

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

A plugin is a folder with two files:

```
link-cleaner/
├── devti-plugin.json   the manifest: who it is and what it may do
└── plugin.wasm         the program, compiled to WebAssembly
```

Add it in **Plugins → Add a plugin**. The Hub shows what it asks for in plain words; nothing
runs until you agree. A working example lives in [`plugins/link-cleaner`](../plugins/link-cleaner):
it removes tracking parameters (`utm_…`, `fbclid`, `gclid`…) from links you copy.

### The sandbox

Plugins run in [Extism](https://extism.org) (wasmtime), one thread per plugin:

| Limit | Value |
|---|---|
| Files, processes, environment | none — WASI is off |
| Memory | 64 MiB |
| Time per call | 2 seconds, then the call is stopped |
| Web requests | only to hosts listed as `http:<host>` permissions |
| Size of `plugin.wasm` | 32 MB |
| Events waiting | 64; a plugin that can't keep up misses events, Devti never waits |

Everything else goes through Devti's host functions, and each one checks the plugin's granted
permissions first. A refused call stops the plugin's call with "this plugin wasn't allowed …",
which appears in its *Recent activity*.

### Manifest

```json
{
  "id": "link-cleaner",
  "name": "Link cleaner",
  "version": "1.0.0",
  "description": "Removes tracking parameters from links you copy.",
  "author": "You",
  "permissions": ["clipboard:read", "clipboard:write", "notify", "events:clipboard"],
  "commands": [{ "id": "clean", "name": "Clean the copied link" }]
}
```

- `id`: 1–40 characters of `a-z 0-9 - _`. Installing a plugin with an existing id updates it.
- `commands`: up to 20, each with a unique `id` and a `name` people see.
- An update that asks for **more** permissions stops until it's turned on again, which
  approves the new list. Unknown permissions are refused outright.

### Permissions

| Permission | What the person is told |
|---|---|
| `notify` | Show notifications on this computer |
| `clipboard:read` | Read this computer's clipboard |
| `clipboard:write` | Change this computer's clipboard (which may then sync to your devices) |
| `devices` | See your devices' names, kinds, battery and whether they're online |
| `send` | Put text on your devices' clipboards |
| `events:device` | Be told when a device connects or disconnects |
| `events:clipboard` | Be told when text is copied on another device — it receives the text |
| `events:notification` | Be told about phone notifications — it receives their text |
| `events:battery` | Be told when a device's battery runs low |
| `http:<host>` | Make web requests to `<host>` (one exact host per permission) |

### What a plugin exports

| Export | Input | Output |
|---|---|---|
| `command` | the command `id` | text shown to the person (Hub, phone remote, rule log) |
| `on_event` | an event as JSON (below) | ignored |

Both are optional. Commands appear in the Hub's command catalog as
`plugin:<plugin id>:<command id>`, so a phone can run them from its remote or deck, and a rule
can run them with *Run a saved command*.

### Events

```json
{ "type": "device", "device": { "id": "…", "name": "Pixel 8", "kind": "phone", "online": true, "battery": 76 } }
{ "type": "clipboard", "from": "…", "from_name": "Pixel 8", "text": "https://…" }
{ "type": "notification", "from": "…", "from_name": "Pixel 8", "app": "Messages", "title": "Ayesha", "text": "…" }
{ "type": "battery", "id": "…", "name": "Pixel 8", "percent": 14 }
```

`device` is sent only when a device connects or disconnects, not for every status update.

### Host functions

All take and return strings (Extism's `ExtismHost` namespace):

| Function | Needs | Input → output |
|---|---|---|
| `devti_log(line)` | — | adds a line to the plugin's *Recent activity* |
| `devti_notify(json)` | `notify` | `{"title","body"}` → `""` |
| `devti_clipboard_get("")` | `clipboard:read` | → the clipboard text |
| `devti_clipboard_set(text)` | `clipboard:write` | → `""` (≤ 256 KB) |
| `devti_devices("")` | `devices` | → a JSON array like the `device` event's |
| `devti_send_text(json)` | `send` | `{"device","text"}` → `""` (lands on that device's clipboard) |

### Writing one in Rust

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
extism-pdk = "1.4"
```

```rust
use extism_pdk::*;

#[host_fn]
extern "ExtismHost" {
    fn devti_clipboard_get(unused: String) -> String;
    fn devti_clipboard_set(text: String) -> String;
}

#[plugin_fn]
pub fn command(id: String) -> FnResult<String> {
    let text = unsafe { devti_clipboard_get(String::new())? };
    unsafe { devti_clipboard_set(text.trim().to_string())? };
    Ok(format!("Ran {id}."))
}
```

```bash
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/your_plugin.wasm plugin.wasm
```

Any language with an Extism PDK works the same way (JavaScript, Go, Zig, C, AssemblyScript…):
import the host functions above from `extism:host/user` and export `command` / `on_event`.

### How it's tested

`plugins/link-cleaner` has unit tests for its link logic, and CI builds it to WebAssembly and
runs it inside the Hub's real sandbox with a stand-in for the computer: the command cleans the
clipboard, an event from another device is cleaned too, and the same plugin with only
`clipboard:read` is refused when it tries to write.
