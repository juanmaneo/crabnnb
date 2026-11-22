# Phone bridge companion (Android)

This Gradle Android application relays crabnnb challenges between the PC (TCP)
and a Garmin watch app through the official Connect IQ Mobile SDK. It binds the
TCP listener to `127.0.0.1:8765`, suitable for local use with `adb forward`.

## Build

Requirements: JDK 17, Gradle 8.9, and Android SDK Platform 35.

```sh
gradle -p phone-bridge-android testDebugUnitTest jacocoTestCoverageVerification assembleDebug
```

`testDebugUnitTest` covers JVM-only TCP framing and the relay's callback,
timeout, device-selection, shutdown, and malformed-payload behavior. The JaCoCo
HTML and XML reports are written to `build/reports/jacoco/jacocoTestReport/`;
`jacocoTestCoverageVerification` gates line, branch, and bytecode-instruction
(region) coverage at 100%. `check` also runs this gate. The debug APK is written
to `phone-bridge-android/build/outputs/apk/debug/`.

Coverage is measured for every JVM-testable production class. The only excluded
production classes are `MainActivity` (Android Activity lifecycle),
`ConnectIqRelayFactory`, and `GarminConnectIqSdk` (Android/Garmin SDK construction
and callback adapters). The relay state machine, protocol, socket server, and
SDK-independent device/relay abstractions are included and held to the 100%
threshold; no app protocol or relay logic is excluded. The Activity and SDK
adapter still need Android/Garmin integration or instrumented tests, and this
unit suite does not exercise Garmin BLE or a physical Connect IQ session.

The app uses Garmin Connect IQ Mobile SDK 2.4.0 from Maven Central. On startup,
it initializes the SDK in wireless mode, requires exactly one connected Garmin
device, registers for the app event before sending the 16-byte challenge, and
returns only a successful 81-byte watch response. Initialization, transfer,
malformed payload, and timeout failures close the TCP client without fabricating
a heartbeat. The SDK must run on a phone with Garmin Connect Mobile installed
and the watch paired and connected. The IQ app ID is
`d72f71b21bf64c49a9ac3c6a670ea1f4`; the Android relay must use the same value.
For store publication, replace it in both projects with the ID assigned by
Garmin's Developer Portal.

The PC protocol uses `u32le length ‖ payload` framing, with 16-byte challenges
and 81-byte heartbeat responses. The watch app compiles against the official
Connect IQ SDK 8.1.1 for FR235; real Garmin Connect Mobile/FR235 hardware
interoperability is not yet verified. JDK 17 is the supported runtime; on the
available JDK 27 environment JaCoCo 0.8.12 may print a non-fatal unsupported-class
warning for a JDK-internal class, but app classes are instrumented and test
coverage verification completes.

## Security/status notes

The local TCP framing is not transport-authenticated or encrypted. Do not
expose the listener on a LAN. A local intermediary can drop/delay frames (denial
of service); heartbeat
verification prevents forging a valid response without the shared key. The
RTT setting is a latency limit, not a physical distance bound.
