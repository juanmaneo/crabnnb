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

Security/status notes: this is not a functional companion yet. The Connect IQ
Mobile SDK callbacks are not implemented; requests fail closed instead of
returning a placeholder heartbeat. The TCP listener binds only to
`127.0.0.1` for use with `adb forward`, and the framing itself is not
transport-authenticated or encrypted. Do not expose it on a LAN. A local
intermediary can drop/delay frames (denial of service); heartbeat verification
prevents forging a valid response without the shared key. The RTT setting is a
latency limit, not a physical distance bound.
