# Phone bridge architecture (Garmin Forerunner 235)

The FR235 cannot host a GATT server (Connect IQ BLE is central-only and not
available on this device), so the secure heartbeat travels:

```
crabnnb (Linux PC)  --TCP-->  phone companion (Android/iOS)  --Garmin Connect / CIQ BLE-->  FR235 IQ app
        ^                        |                                   |
        +--- encrypted heartbeat -+--- challenge (16B) -------------+
```

- PC: `crabnnb <watch-id> --key-file /etc/crabnnb/key.hex --bridge-addr 127.0.0.1:8765`
  (`--bridge-addr` makes the PC *connect* to the bridge; run the phone
  companion's TCP server locally, e.g. via `adb forward tcp:8765 tcp:8765`).
- Bridge framing on TCP: `u32le length ‖ payload`. PC→phone payload = 16-byte
  random challenge; phone→PC payload = 81-byte heartbeat (mode 1, below).
- Phone companion: `phone-bridge-android/` (skeleton) using the Connect IQ
  Mobile SDK; relays frames between TCP and the watch app.
- Watch app: `iq-phone-bridge/` (Monkey C), answers each challenge with an
  encrypted heartbeat; counter persisted in `Toybox.Storage`.

## Bridge heartbeat format (mode 1, matches `src/heartbeat.rs` MODE_AES_CBC_HMAC)

Monkey C has AES + SHA-256 but no AEAD, so the bridge uses encrypt-then-MAC:

```
msg(81) = 0x01 ‖ iv(16) ‖ AES-256-CBC-PKCS7(pt)(32) ‖ HMAC-SHA256(macKey, msg[0..49])(32)
pt(24)  = counter u32le ‖ unix-seconds u32le ‖ challenge(16)
macKey  = SHA256("crabnnb-mac" ‖ key)
iv      = HMAC-SHA256(macKey, pt)[0..16]        (unique per message, no RNG needed)
key     = the same 32-byte key as the PC --key-file (entered as 64 hex chars
          in the Garmin Connect app settings — note this transits Garmin's
          cloud once at provisioning; sideload a baked-in key for stricter setups)
```

The PC verifies, in order: HMAC tag (constant-time), AES-CBC padding/decrypt,
challenge echo, strictly increasing counter, timestamp within
`--heartbeat-window-ms`. Any failure → `Sample::Unverified` → locks after
`--max-missed` polls. A Flipper Zero or a compromised phone alone cannot
produce a valid tag without the key.

Direct-BLE mode 0 (ChaCha20-Poly1305, 60/61-byte messages) is unchanged and
still used when the watch/peripheral can host GATT.

## Relay-attack mitigation

1. **RTT bound (implemented, default on)**: every heartbeat must arrive within
   `--max-roundtrip-ms` (default 250 ms; `0` disables) of the challenge being
   sent. Because the heartbeat must echo the fresh challenge, the measured
   round trip covers the whole path PC→phone→watch→phone→PC; any relay hop
   (RF or internet backhaul) adds latency and gets rejected as
   `ProbeError::TooSlow`. Calibrate with `--once --dry-run --verbose` (prints
   measured RTT). In bridge mode there is no RSSI, so this bound *is* the
   distance bound; in direct-BLE mode it complements `--max-distance-cm`.
2. **RSSI second factor (direct BLE only)**: unlock additionally requires the
   RSSI-derived distance below `--max-distance-cm`. Note a relay attacker near
   the PC reads strong RSSI, so RSSI stops non-relay spoofing, not relays —
   the RTT bound and the AEAD tag are what stop relays/spoofing.
3. **Hardware distance bounding (strongest)**: BLE Channel Sounding (BT 6.0,
   phase-based ranging, sub-meter) or UWB (e.g. DW3000 dongles, FiRa) between
   PC and watch. Requires capable radios on both ends (not available on the
   FR235); when available, enforce a physical distance ceiling on top of the
   crypto heartbeat. Check controller support with `bluetoothctl` /
   `hcitool` LE CS capabilities.
4. **Link-layer encryption**: pair the devices (LE Secure Connections) as
   defense-in-depth; the heartbeat carries no secrets, so the AEAD/HMAC tag is
   the primary spoof defense even on an unencrypted link.

## Testing without hardware

`cargo test` covers the bridge mode end-to-end against a local TCP mock
(valid heartbeat unlocks; a 50 ms-delayed "relay" response is rejected by the
RTT bound). The IQ app and Android companion are skeletons to compile with the
respective Garmin SDKs.
