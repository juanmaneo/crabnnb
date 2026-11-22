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

Install the Connect IQ SDK and configure its `monkeyc` compiler. The included
`monkey.jungle` defines the app's source, resources, and manifest. Garmin's
Connect IQ SDK requires an **RSA 4096-bit** developer private key; a 2048-bit
key produces an app the FR235 rejects with `Signature check failed`. Generate
a key if you do not already have a 4096-bit one, and keep it private and backed
up (do not commit it):

```sh
openssl genrsa -out "$HOME/.Garmin/ConnectIQ/keys/crabnnb-developer.pem" 4096
openssl pkey -in "$HOME/.Garmin/ConnectIQ/keys/crabnnb-developer.pem" \
  -outform DER -out "$HOME/.Garmin/ConnectIQ/keys/crabnnb-developer.der"
chmod 600 "$HOME/.Garmin/ConnectIQ/keys/crabnnb-developer.pem" \
  "$HOME/.Garmin/ConnectIQ/keys/crabnnb-developer.der"
```

From this directory, compile for the Forerunner 235 using the DER key:

```sh
mkdir -p build
monkeyc -f monkey.jungle -o build/PhoneBridge.prg -d fr235 -y "$HOME/.Garmin/ConnectIQ/keys/crabnnb-developer.der"
```

Keep using the same key for future builds/updates; replacing it changes the
app's signing identity.

The app ID is `d72f71b21bf64c49a9ac3c6a670ea1f4`; the Android relay is configured
with the same ID. This ID is for this sideloadable project; use the ID assigned
by the Garmin Developer Portal instead if publishing, and update both projects.

Sideload the resulting `.prg` onto the watch (`GARMIN/APPS/`). The app compiles
for `fr235` with Connect IQ SDK 8.1.1. Compilation reports deprecation warnings
for the API 1.4 app-property calls; actual FR235/phone interoperability remains
untested.

Set the **pairing key** in Garmin Connect (app settings) to the same 64 hex
characters as the PC key file (`crabnnb --gen-key-file /etc/crabnnb/key.hex`).
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

- Communications delegate and crypto calls compile with Connect IQ SDK 8.1.1
  for `fr235`, but callback behavior and cryptographic output still need
  end-to-end validation against Garmin Connect Mobile and a physical watch.
- The watch clock on the FR235 is set from the phone; `System.getTimer()` is
  uptime, not wall-clock — use `System.getClockTime()` (wall clock) for the
  timestamp instead. The PC accepts timestamps within
  `--heartbeat-window-ms` (default 5000 ms).
- See `../docs/phone-bridge.md` for the full architecture and the PC/phone
  companion side.
