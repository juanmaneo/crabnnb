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

1. **Phone bridge** (works today): a Connect IQ app on the FR235 uses
   `Toybox.Communications` to send an authenticated heartbeat to a phone
   companion (Garmin Connect Mobile), which relays it to the PC over the local
   network (TLS). The crypto/heartbeat modules in this repo
   (`crypto.rs`, `heartbeat.rs`, `session.rs`) are transport-agnostic and can
   be reused verbatim over that channel.
2. **ANT+ channel**: the FR235 supports ANT+; a CIQ app can broadcast a heartbeat
   payload on an ANT channel to an ANT+ USB stick on the Linux PC. Same crypto,
   different transport.
3. **Newer watch**: on CIQ 3.1+ devices the watch can act as BLE *central*; run
   a BlueZ GATT server on the Linux PC and let the watch connect to it. Again the
   same protocol, with roles swapped.
4. Any BLE peripheral that runs the reference responder (e.g. an nRF52/ESP32
   bridge paired to the watch) also works with the PC-side code as-is.

For any of these, the guarantee is identical: **unlock requires a fresh
ChaCha20-Poly1305-authenticated heartbeat under the pre-shared key plus
proximity**, so a Flipper Zero replaying or spoofing advertisements cannot
unlock the session.

## Linux PC notes

* Linux only (raw HCI sockets, no libbluetooth). BlueZ (`bluetoothd`) must be
  running and the adapter up (`bluetoothctl power on`).
* HCI raw channel (`HCI_CHANNEL_RAW`) requires `CAP_NET_ADMIN`. Prefer a
  systemd unit with `AmbientCapabilities=CAP_NET_ADMIN` +
  `CapabilityBoundingSet=CAP_NET_ADMIN` instead of running as full root.
* The key file must be mode `0600` (enforced at load time).

## Residual risks

* **Relay attacks**: an attacker with two radios between you and the PC could
  forward the watch's heartbeat while you are away. The freshness window and
  challenge-response bound this to `--heartbeat-window-ms`, but do not fully
  eliminate it. True mitigation is distance bounding (BLE Channel Sounding /
  UWB) or the RSSI second factor (attacker near the PC reads strong RSSI, so
  keep `--max-distance-cm` tight). For most threat models (Flipper Zero in a
  pocket) the AEAD heartbeat is decisive.
* **Key compromise**: if the key file leaks, revoke by generating a new key and
  re-provisioning the watch side.
* BLE link-layer encryption (pairing / LE Secure Connections) is recommended as
  defense-in-depth; the heartbeat itself carries no secrets, so the AEAD tag is
  what defeats spoofing even on an unencrypted link.
