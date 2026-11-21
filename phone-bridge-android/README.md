# Phone bridge companion (Android)

This Gradle Android application relays crabnnb challenges between the PC (TCP)
and a Garmin watch app through the official Connect IQ Mobile SDK. It binds the
TCP listener to `127.0.0.1:8765`, suitable for local use with `adb forward`.

## Build

Requirements: JDK 17, Gradle 8.9, and Android SDK Platform 35.

```sh
gradle -p phone-bridge-android testDebugUnitTest assembleDebug
```

`testDebugUnitTest` covers JVM-only TCP framing and the relay's callback,
timeout, device-selection, and malformed-payload behavior. It does not exercise
Garmin BLE or a physical Connect IQ session. The debug APK is written to
`phone-bridge-android/build/outputs/apk/debug/`.

The app uses Garmin Connect IQ Mobile SDK 2.4.0 from Maven Central. On startup,
it initializes the SDK in wireless mode, requires exactly one connected Garmin
device, registers for the app event before sending the 16-byte challenge, and
returns only a successful 81-byte watch response. Initialization, transfer,
malformed payload, and timeout failures close the TCP client without fabricating
a heartbeat. The SDK must run on a phone with Garmin Connect Mobile installed
and the watch paired and connected; the IQ app ID must match
`crabnnb-phone-bridge`.

The PC protocol uses `u32le length ‖ payload` framing, with 16-byte challenges
and 81-byte heartbeat responses. The SDK integration has not yet been verified
against real Garmin Connect Mobile/FR235 hardware, and the IQ watch app still
needs compilation against the official Connect IQ SDK.

## Security/status notes

The local TCP framing is not transport-authenticated or encrypted. Do not
expose the listener on a LAN. A local intermediary can drop/delay frames (denial
of service); heartbeat
verification prevents forging a valid response without the shared key. The
RTT setting is a latency limit, not a physical distance bound.
