# Phone bridge companion (Android skeleton)

This is a Gradle Android application skeleton for relaying crabnnb challenges
between the PC (TCP) and the watch IQ app (Garmin Connect). It currently binds
the TCP listener to `127.0.0.1:8765`, suitable for local use with `adb forward`.

## Build

Requirements: JDK 17, Gradle 8.9, and Android SDK Platform 35.

```sh
gradle -p phone-bridge-android testDebugUnitTest assembleDebug
```

`testDebugUnitTest` covers JVM-only TCP framing and fail-closed relay behavior;
it does not exercise Garmin BLE or Connect IQ Mobile SDK integration. The debug
APK is written to `phone-bridge-android/build/outputs/apk/debug/`.

The Android app has an explicit `ConnectIqRelay` adapter boundary. The
`ConnectIqMobileSdkStub` currently throws an error for every request, so the
app fails closed and does not return a fabricated heartbeat. The Garmin SDK
artifact and callbacks are intentionally not guessed: install the official
Connect IQ Mobile SDK and replace the stub with an implementation matching the
installed SDK's API before expecting a working relay. The PC protocol uses
`u32le length ‖ payload` framing, with 16-byte challenges and 81-byte
heartbeat responses.

## Security/status notes

This is not a functional companion yet. The local TCP framing is not
transport-authenticated or encrypted. Do not expose the listener on a LAN. A
local intermediary can drop/delay frames (denial of service); heartbeat
verification prevents forging a valid response without the shared key. The
RTT setting is a latency limit, not a physical distance bound.
