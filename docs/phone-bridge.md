# Phone bridge architecture (Garmin Forerunner 235)

The FR235 cannot host a GATT server (Connect IQ BLE is central-only and not
available on this device), so the secure heartbeat travels:

```
crabnnb (Linux PC)  --TCP-->  phone companion (Android/iOS)  --Garmin Connect / CIQ BLE-->  FR235 IQ app
        ^                        |                                   |
        +--- encrypted heartbeat -+--- challenge (16B) -------------+
```

- PC: `crabnnb AA:BB:CC:DD:EE:FF --key-file /etc/crabnnb/key.hex --bridge-addr 127.0.0.1:8765`
  (`--bridge-addr` makes the PC *connect* to the bridge; the positional value
  must still be a syntactically valid MAC address. Run the phone companion's
  TCP server locally, e.g. via `adb forward tcp:8765 tcp:8765`).
- Bridge framing on TCP: `u32le length ‖ payload`. PC→phone payload = 16-byte
  random challenge; phone→PC payload = 81-byte heartbeat (mode 1, below).
- Phone companion: `phone-bridge-android/` is an incomplete skeleton. Its local
  TCP framing is specified, but the Connect IQ Mobile SDK callbacks are not
  implemented; it cannot currently relay a challenge or return a heartbeat.
- Watch app: `iq-phone-bridge/` is an unverified prototype. Its Monkey C SDK
  calls and crypto APIs have not been compiled or tested on an FR235.

## Bridge heartbeat format (mode 1, matches `src/heartbeat.rs` MODE_AES_CBC_HMAC)

Monkey C has AES + SHA-256 but no AEAD, so the bridge uses encrypt-then-MAC:

```
msg(81) = 0x01 ‖ iv(16) ‖ AES-256-CBC-PKCS7(pt)(32) ‖ HMAC-SHA256(macKey, msg[0..49])(32)
pt(24)  = counter u32le ‖ unix-seconds u32le ‖ challenge(16)
macKey  = SHA256("crabnnb-mac" ‖ key)
iv      = HMAC-SHA256(macKey, pt)[0..16]        (unique per message, no RNG needed)
key     = the same 32-byte key as the PC --key-file. The prototype describes
          entering 64 hex chars in Garmin Connect app settings; this may send
          the key through Garmin's cloud and is not suitable for high-security
          deployments. Prefer a verified out-of-band method before using this.
```

The PC verifies, in order: HMAC tag (constant-time), AES-CBC padding/decrypt,
challenge echo, strictly increasing counter, timestamp within
`--heartbeat-window-ms`. Any failure → `Sample::Unverified` → locks after
`--max-missed` polls. A Flipper Zero or a compromised phone alone cannot
produce a valid tag without the key.

Direct-BLE mode 0 (ChaCha20-Poly1305, 60/61-byte messages) is unchanged and
still used when the watch/peripheral can host GATT.

## Relay attacks and round-trip latency

`--max-roundtrip-ms` (default 250 ms; `0` disables) rejects heartbeats that
arrive too long after their fresh challenge. This is an application-level
latency policy, not a physical distance bound: a relay whose total response
time stays below the configured threshold can still pass. A slow legitimate
phone/watch path can also be rejected. Calibrate with `--once --dry-run
--verbose`, but do not interpret an accepted RTT as proof that the watch is
nearby. In bridge mode there is no RSSI or hardware ranging, so cryptographic
authentication proves possession of the key, not physical proximity.

In direct-BLE mode, RSSI below `--max-distance-cm` is an additional noisy
signal, not secure distance bounding. A relay near the PC can present strong
RSSI while forwarding a real watch response. Stronger relay resistance needs
hardware distance bounding (for example suitable UWB or BLE Channel Sounding
hardware); the FR235/phone bridge does not provide that. Link-layer encryption
is defense-in-depth but does not establish physical proximity.

## Current implementation status and testing

Rust tests cover bridge framing, heartbeat verification, and the configured
RTT timeout against a local TCP mock. That mock only checks that a response
slower than the configured policy is rejected; it does not demonstrate relay
resistance or physical distance bounding.

The Android companion is not a working relay yet: it has no Gradle project or
Connect IQ Mobile SDK integration, and its relay callback is deliberately
fail-closed. The TCP listener binds to `127.0.0.1` only and the transport has no
authentication/encryption; do not expose it on a LAN. A local intermediary can
cause denial of service, though it cannot forge a valid heartbeat without the
key. The Connect IQ app is also uncompiled and untested; its SDK calls and
crypto API usage need validation against the installed SDK and FR235. Do not
deploy the phone bridge as an operational security feature until both apps are
completed and tested end-to-end on hardware.
