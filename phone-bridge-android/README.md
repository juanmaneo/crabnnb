# Phone bridge companion (Android skeleton)

Relays crabnnb challenge/response between the PC (TCP) and the watch IQ app
(Connect IQ Mobile SDK over Garmin Connect). Build with the Garmin Connect IQ
Mobile SDK (`com.garmin.connectiq:connectiq-mobile-sdk`); the exact SDK
entry points are marked with TODO and must be matched to the installed SDK:

- `ConnectIQ.getInstance(context).registerForApplicationMessages(iqAppId, listener)`
  to receive heartbeats from `crabnnb-phone-bridge`
- `sendMessage(iqAppId, ByteArray)` to forward the PC's 16-byte challenge
- a TCP server on port 8765 (or `adb forward tcp:8765 tcp:8765` to this app)
  speaking the crabnnb bridge framing: `u32le length ‖ payload`

See `src/main/java/com/example/crabnnbbridge/MainActivity.kt` for the loop.

Security notes: the bridge only carries the random challenge and the encrypted
heartbeat — no key material crosses it. Restrict the TCP listener to localhost
(adb forward) or your LAN; use TLS if the phone and PC are on untrusted
networks. The PC side still enforces the AEAD/HMAC tag, replay counter,
freshness window and `--max-roundtrip-ms` RTT bound, so a compromised phone
cannot unlock the PC on its own.
