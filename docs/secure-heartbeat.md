# CRABNNB secure heartbeat protocol

This document specifies the encrypted, authenticated heartbeat that replaces
MAC+RSSI-only presence detection as the basis for unlock decisions.

## Threat model

Before this change, presence was decided by two spoofable signals:

1. **BD_ADDR match** — a Flipper Zero (or any BLE radio) can claim the watch's
   MAC address and answer the HCI Remote Name Request.
2. **RSSI** — signal strength is fully controlled by whoever transmits. A
   nearby attacker with high TX power reads as "0 cm away".

Result: one spoofed advertisement cycle was enough to trigger `unlock-cmd`
(and any user-defined command attached to it).

The secure heartbeat adds a third factor that cannot be forged without the
pre-shared key: a **challenge-response AEAD message** that the real watch must
produce on every poll.

## Key provisioning

```
crabnnb --gen-key-file /etc/crabnnb/key.hex     # written with mode 0600, never overwritten
```

The file contains 64 hex chars (32 bytes). `crabnnb --key-file` refuses to load
a key file that is readable by group/others. Distribute the key to the watch
side out-of-band (USB, QR, manual entry into the watch app) — never over BLE.

## Channel

* PC role: BLE central + GATT client. After the ACL link exists, the PC opens
  the ATT fixed channel (L2CAP CID `0x0004`) and exchanges MTU (request 247;
  the heartbeat needs MTU >= 63).
* The watch side exposes one GATT service with two characteristics:
  * **challenge characteristic** (write): PC writes a fresh 16-byte random
    nonce every poll. Default ATT handle `0x0031` (`--challenge-handle`).
  * **heartbeat characteristic** (notify): watch answers with the heartbeat
    message on the handle `0x0034` (`--heartbeat-handle`).
* RSSI (`HCI Read RSSI`) is still read over the same ACL link and used as a
  second factor: unlock requires a **verified heartbeat AND distance below
  `--max-distance-cm`**. A missing/invalid heartbeat counts as "away" and
  locks after `--max-missed` polls.

## Message format

Challenge (PC -> watch), written to the challenge characteristic:

```
challenge = 16 random bytes
```

Heartbeat (watch -> PC), notified on the heartbeat characteristic (60 bytes):

```
offset  len  field
0       12   nonce (random per heartbeat)
12      48   ChaCha20-Poly1305 ciphertext || 16-byte tag
```

Plaintext (32 bytes, before encryption):

```
offset  len  field
0       8    counter, u64 LE, strictly increasing (persisted on the watch)
8       8    unix timestamp milliseconds, u64 LE
16      16   echo of the outstanding challenge
```

AAD (12 bytes): `watch_bdaddr (6) || pc_bdaddr (6)` — binds every heartbeat to
this PC and this watch.

Algorithm: ChaCha20-Poly1305 (RFC 8439), 256-bit key, 96-bit nonce.

## Verification rules (PC side, `heartbeat::open_heartbeat`)

A heartbeat is accepted only if ALL of the following hold:

1. Length is exactly 60 bytes and the AEAD tag verifies under the key and AAD
   (rejects forgery and tampering — a Flipper cannot produce this without the
   key).
2. The echoed challenge equals the nonce the PC just sent (rejects
   pre-computed or recorded responses).
3. `counter > last_counter` (rejects replays; the watch must persist the
   counter across reboots, e.g. in Connect IQ `Storage`).
4. `|now_ms - timestamp_ms| <= --heartbeat-window-ms` (rejects stale and
   far-future timestamps; default 5000 ms).

Only then does the poll produce `Sample::Distance(rssi)`; otherwise it
produces `Sample::Unverified`, which the `Monitor` treats exactly like "away"
and locks after `--max-missed` consecutive polls.

## Garmin Forerunner 235 note

Connect IQ **cannot host a GATT server**: `Toybox.BluetoothLowEnergy` is
central-role only, and it exists only on Connect IQ 3.1+ devices (Forerunner
245/645/945, fenix 5 Plus, etc.) — the Forerunner 235 predates it. So a custom
watch app on the FR235 cannot be the GATT server that this PC-side client
expects. Practical options for the FR235:

1. **Phone bridge** (implemented, not yet hardware-validated): the Connect IQ
   Mobile SDK Android companion now relays challenges and accepts only a
   successful 81-byte response from the matching app and connected watch. Its JVM
   tests cover callback and failure behavior, but not Garmin BLE. The watch app
   has not been compiled or tested against the FR235 SDK; do not treat the bridge
   as operational until both sides are built and validated on hardware.
2. **ANT+ channel**: the FR235 supports ANT+; a CIQ app can broadcast a heartbeat
   payload on an ANT channel to an ANT+ USB stick on the Linux PC. Same crypto,
   different transport.
3. **Newer watch**: on CIQ 3.1+ devices the watch can act as BLE *central*; run
   a BlueZ GATT server on the Linux PC and let the watch connect to it. Again the
   same protocol, with roles swapped.
4. Any BLE peripheral that runs the reference responder (e.g. an nRF52/ESP32
   bridge paired to the watch) also works with the PC-side code as-is.

For a compatible, tested responder, unlock requires a fresh authenticated
heartbeat under the pre-shared key. This prevents a device without the key
from forging a valid response, but the heartbeat alone does not prove physical
proximity and does not prevent a sufficiently fast relay. Proximity requires a
separate, appropriately validated ranging mechanism.

## Linux PC notes

* Linux only (raw HCI sockets, no libbluetooth). BlueZ (`bluetoothd`) must be
  running and the adapter up (`bluetoothctl power on`).
* HCI raw channel (`HCI_CHANNEL_RAW`) requires `CAP_NET_ADMIN`. Prefer a
  systemd unit with `AmbientCapabilities=CAP_NET_ADMIN` +
  `CapabilityBoundingSet=CAP_NET_ADMIN` instead of running as full root.
* The key file must be mode `0600` (enforced at load time).

## Relay attacks and proximity limits

`--max-roundtrip-ms` (default 250 ms; `0` disables) is an application-level
latency limit. It rejects responses arriving after the threshold, but it is
not a physical distance bound: a relay that completes within the threshold can
still pass, and a slow legitimate path can be rejected. In bridge mode, an
accepted heartbeat proves key possession and challenge freshness, not that
the watch is nearby. The local TCP bridge also has no transport authentication
or encryption; keep it on loopback/trusted links. An intermediary can at least
drop or delay frames and cause denial of service.

In direct BLE mode, RSSI is an additional noisy signal, not cryptographic
distance bounding. A relay close to the PC can produce strong RSSI. Stronger
relay resistance needs suitable physical-layer distance-bounding hardware (for
example UWB or supported BLE Channel Sounding) and end-to-end validation.
BLE link-layer encryption is useful defense-in-depth but does not by itself
prove proximity.

## Residual risks

* Application-layer RTT bounding is coarse compared to physical-layer distance
  bounding; a very fast relay could theoretically stay under the bound — hence
  keep `--max-roundtrip-ms` as tight as your link allows and prefer CS/UWB on
  capable hardware.
* **Key compromise**: if the key file leaks, revoke by generating a new key and
  re-provisioning the watch side.
* The heartbeat carries no secrets, so the AEAD/HMAC tag is what defeats
  spoofing even on an unencrypted link.

## Phone bridge prototype

Devices that cannot host a GATT server (e.g. Forerunner 235) are intended to
use `--bridge-addr HOST:PORT`, with a phone relaying TCP frames to a watch IQ
app over `Toybox.Communications`. The Android companion includes a Connect IQ
Mobile SDK relay with fail-closed handling; the watch app compiles for FR235 with
Connect IQ SDK 8.1.1. End-to-end Garmin Connect Mobile/FR235 behavior remains
untested. The bridge heartbeat format uses mode 1 (AES-256-CBC + HMAC-SHA256
encrypt-then-MAC, 81 bytes). See [phone-bridge.md](phone-bridge.md) for current
status and wire format. Do not use this prototype as an operational security
control until validated on hardware.
