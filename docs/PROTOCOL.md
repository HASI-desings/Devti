# Devti protocol — version 1

Normative description of how Devti devices find, authenticate and talk to each other. Code:
`core/src/protocol`, `core/src/pairing`, `core/src/node`.

## 1. Transport

- **QUIC v1** (RFC 9000) over UDP, default port **47810** (a random port is used if it's taken; the
  real port is advertised by mDNS and in the QR link).
- **TLS 1.3 only**, cipher suites from rustls/ring, **ALPN `devti/1`**, server name `devti` (ignored).
- **Mutual authentication is mandatory.** Both sides present a self-signed X.509 certificate
  holding an **Ed25519** key.
- Keep-alive every 4 s, idle timeout 15 s.

### Identity
- `fingerprint = SHA-256(SubjectPublicKeyInfo)` (32 bytes).
- `device_id = Base32(fingerprint[0..10])`, 16 characters, no padding.
- Certificate validity dates are not checked; trust is the pinned fingerprint.

### Admission
| Server state | Client fingerprint trusted | Client unknown |
|---|---|---|
| Normal | accept | **reject in TLS handshake** |
| Pairing window open | accept | accept, but only a `pair` stream is allowed |
| Paused | reject | reject |

Clients pin the server's fingerprint: from the trust store for known devices, from the QR link
when pairing by QR. When pairing by typed code the server fingerprint is unknown and the PAKE
transcript authenticates it instead (§4).

## 2. Framing

```
frame = length:u32be  body:CBOR(message)
```
- Limits: 16 KiB on `pair` streams, 1 MiB on `control`, 16 MiB absolute; nesting depth ≤ 64.
- A frame over its limit closes the stream with a protocol error.

## 3. Streams

Every bidirectional stream starts with a `StreamOpen` frame:

```cddl
StreamOpen = { "stream": "pair" / "control" / tstr }   ; unknown → stream reset
```

### Control stream (one per session, opened by the dialler)

```cddl
ControlMsg = Hello / Status / Info / Ping / Pong / Goodbye
Hello   = { "t": "hello",   "proto": 1, "info": DeviceInfo, "status": DeviceStatus }
Status  = { "t": "status",  "status": DeviceStatus }
Info    = { "t": "info",    "info": DeviceInfo }
Ping    = { "t": "ping",    "nonce": uint }
Pong    = { "t": "pong",    "nonce": uint }
Goodbye = { "t": "goodbye", "reason": "shutdown" / "revoked" / "paused" / tstr }

DeviceInfo = {
  "id": tstr, "name": tstr, "kind": "desktop"/"laptop"/"phone"/"tablet"/"accessory"/tstr,
  "platform": "windows"/"linux"/"macos"/"android"/"ios"/"ipados"/tstr,
  ? "os_version": tstr, ? "model": tstr / null, ? "app_version": tstr, ? "capabilities": [* tstr]
}
DeviceStatus = { ? "battery": { "percent": 0..100, "charging": bool } / null }
```

Rules:
- The dialler sends `Hello` first and the acceptor replies with its own. `info.id` **must** equal
  the ID derived from the peer's TLS certificate, otherwise the connection is closed.
- `proto` must be 1. Unknown message types are ignored.
- Receivers sanitize all strings (length caps; control, zero-width and bidi characters removed).
- **One session per pair of devices.** If two connections exist, keep the one initiated by
  `min(id_a, id_b)` and close the other with code `DUPLICATE`.

### Close codes
`0` normal · `1` untrusted · `2` duplicate · `3` revoked · `4` paused · `5` shutdown ·
`6` protocol error · `7` pairing finished · `8` pairing failed.
Receiving `3` (revoked) means the other side removed you: delete it from your trust store as well.

## 4. Pairing

### 4.1 Pairing window
The hub opens a window (5 minutes; at most one active pairing; 3 failed attempts close it) holding:
- a 128-bit **QR secret**, and
- a uniformly random **6-digit code** (rejection-sampled).

### 4.2 QR link
```
devti://pair?v=1&id=<device_id>&fp=<base64url(fingerprint)>&s=<base32(secret)>
            &p=<port>&a=<ip>[,<ip>…≤8]&n=<name>&k=<kind>
```
Strictly validated, ≤ 1024 bytes; `id` must match `fp`. Unknown parameters are ignored.

### 4.3 Exchange (on a `pair` stream, joiner = client)

```
Joiner → Hub   Start   { "t":"start", "proto":1, "method":"qr"/"code", "spake": bstr, "info": DeviceInfo }
Hub → Joiner   Reply   { "t":"reply", "spake": bstr, "info": DeviceInfo }
Joiner → Hub   Confirm { "t":"confirm", "mac": bstr }
Hub → Joiner   Confirm { "t":"confirm", "mac": bstr }
both ↔         Decision{ "t":"decision", "accept": bool }      ; after the user compares the SAS
Hub → Joiner   Done    { "t":"done" }                          ; hub has stored the joiner
either         Abort   { "t":"abort", "reason": "bad_secret"/"rejected"/"not_accepting"/"busy"/
                                                 "incompatible"/"identity_mismatch"/"timeout"/tstr }
```

### 4.4 Key schedule
```
password    = "qr:" ‖ secret          (method qr)
            | "code:" ‖ ascii_digits  (method code)
K           = SPAKE2-Ed25519(password, idA = "devti-hub", idB = "devti-joiner")
exporter    = TLS-Exporter(label = "EXPORTER-devti-pairing", context = "devti", 32)
transcript  = "devti-pair-v1" ‖ exporter ‖ fp_hub ‖ fp_joiner
              ‖ u32be(len A) ‖ msgA ‖ u32be(len B) ‖ msgB
prk         = HKDF-Extract(salt = SHA-256(transcript), ikm = K)
k_hub       = HKDF-Expand(prk, "devti hub confirm", 32)
k_joiner    = HKDF-Expand(prk, "devti joiner confirm", 32)
SAS         = u32be(HKDF-Expand(prk, "devti sas", 4)) mod 1 000 000      (shown "ddd ddd")
mac_joiner  = HMAC-SHA256(k_joiner, transcript)
mac_hub     = HMAC-SHA256(k_hub, transcript)
```
- The hub verifies `mac_joiner` before revealing `mac_hub`. A failure counts as a wrong secret.
- Both users must confirm that the SAS matches. A rejection on either side aborts.
- Only after both `Decision{accept:true}` does the hub store the joiner and send `Done`. The
  joiner then stores the hub and opens the `control` stream **on the same connection**.

### 4.5 Security properties
- **Offline guessing:** impossible. An eavesdropper learns nothing about the code (PAKE).
- **Online guessing:** one guess per attempt, 3 attempts per window: ≤ 3·10⁻⁶ with a code, and
  2⁻¹²⁸ with a QR code.
- **Man in the middle:** the two TLS legs have different exporters and fingerprints, so the
  confirmation MACs fail. Even if someone photographed the QR secret, the SAS check still exposes
  them.
- **Identity binding:** `info.id` in `Start`/`Reply` must match the TLS certificate, otherwise
  `identity_mismatch`.

## 5. Discovery

mDNS/DNS-SD service `_devti._udp.local.`, instance = device ID, port = QUIC port.
TXT: `v=1`, `id`, `n` (name), `k` (kind), `p` (platform), `f` (base64url fingerprint),
`po` (`1` while a pairing window is open). Discovery results are hints. Only TLS decides identity.

## 6. Host API (JSON)

Desktop (Tauri events `devti://event`) and mobile (UniFFI `EventListener`) receive the same
`NodeEvent` JSON, tagged by `"type"`:
`local_changed`, `peer_changed`, `peer_removed`, `device_discovered`, `device_lost`,
`pairing_window_changed`, `pairing_started`, `pairing_verify`, `pairing_completed`,
`pairing_failed`, `paused` (and `resync` when a listener fell behind). Shapes:
`core/src/node/events.rs`, mirrored in `desktop/src/bridge/types.ts` and
`android/.../core/state/Models.kt`. Pairing IDs are below 2⁵³ so they survive JavaScript numbers.
