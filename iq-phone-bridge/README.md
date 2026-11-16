# CRABNNB phone-bridge IQ app (Garmin Forerunner 235)

This is the watch-side responder for crabnnb's phone-bridge mode. The FR235
cannot host a GATT server (Connect IQ `Toybox.BluetoothLowEnergy` is
central-only and unavailable on this device), so the watch instead answers
the PC's challenge over `Toybox.Communications` through the Garmin Connect
phone, and the phone companion relays to the PC over TCP.

Flow: `crabnnb --bridge-addr <phone-host>:8765 --key-file key.hex` → TCP →
phone companion → Garmin Connect / CIQ BLE → this app → encrypted heartbeat
back the same way.

## Build

1. Install the Connect IQ SDK + Monkey C VS Code extension.
2. Open this folder, build for `forerunner235`, sideload the `.prg` onto the
   watch (`GARMIN/APPS/`).
3. Set the **pairing key** in Garmin Connect (app settings) to the same 64 hex
   chars as the PC key file (`crabnnb --gen-key-file /etc/crabnnb/key.hex`).
   Note: app settings set via Garmin Connect transit Garmin's cloud once —
   acceptable for most threat models; for stricter provisioning, bake the key
   into `resources/properties.xml` and sideload.

## Protocol (must match crabnnb `src/heartbeat.rs`, MODE_AES_CBC_HMAC)

- Phone→watch message: 16-byte random challenge from the PC.
- Watch→phone message (81 bytes): `0x01 ‖ iv(16) ‖ AES-256-CBC-PKCS7(counter u32le ‖ unix-sec u32le ‖ challenge)(32) ‖ HMAC-SHA256(macKey, first 49 bytes)(32)`
- `macKey = SHA256("crabnnb-mac" ‖ key)`, `iv = HMAC-SHA256(macKey, plaintext)[0..16]`.
- The monotonic counter is persisted in `Toybox.Storage` so reboots cannot
  trigger replay acceptance on the PC.

## Caveats

- Monkey C API details (Communications delegate method names, Digest/Cipher
  signatures) should be verified against the installed SDK version; the logic
  and wire format are the contract with crabnnb.
- The watch clock on the FR235 is set from the phone; `System.getTimer()` is
  uptime, not wall-clock — use `System.getClockTime()` (wall clock) for the
  timestamp instead. The PC accepts timestamps within
  `--heartbeat-window-ms` (default 5000 ms).
- See `../docs/phone-bridge.md` for the full architecture and the PC/phone
  companion side.
