# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security
- Presence is no longer decided by MAC address + RSSI alone (both spoofable, e.g. with a Flipper Zero). Each poll now requires a challenge-response **encrypted heartbeat** from the device: the PC writes a fresh 16-byte challenge over ATT and the device must reply with a ChaCha20-Poly1305 (RFC 8439) authenticated message (monotonic counter + unix-ms timestamp + challenge echo, AAD = watch and PC MACs). Replay, stale/future timestamps, wrong challenge and tag mismatches are all rejected. Unlock additionally requires RSSI distance below the threshold; a failed heartbeat counts as "away" and locks after `--max-missed` polls.
- Secure mode is the default: running without `--key-file` is refused unless `--allow-insecure` is passed explicitly (legacy mode now prints a spoofability warning).
- Key files are provisioned with `--gen-key-file` (random 32 bytes, mode 0600, never overwritten) and rejected at load time if readable by group/others.

### Added
- Challenge RTT policy: `--max-roundtrip-ms` rejects slow responses and reports measured RTT in verbose mode. This is not physical distance bounding; a relay that responds under the threshold may pass.
- Phone-bridge transport for devices that cannot host GATT (e.g. Forerunner 235): `--bridge-addr HOST:PORT` connects to a phone companion over TCP with `u32le len ‖ payload` framing; new `src/bridge.rs` (`BridgeDevice` implements `BtOps`)
- Second heartbeat wire mode for constrained devices: `MODE_AES_CBC_HMAC` (81 bytes: `0x01 ‖ iv ‖ AES-256-CBC-PKCS7(counter‖ts‖challenge) ‖ HMAC-SHA256`) — encrypt-then-MAC using AES/SHA-256, available in Monkey C's `Toybox.Cryptography`; verified with HMAC RFC 4231 and NIST AES-256-CBC vectors
- `iq-phone-bridge/`: uncompiled Connect IQ watch-app prototype for the FR235; SDK calls and crypto API are not hardware-validated
- `phone-bridge-android/`: incomplete Android companion skeleton; TCP framing is present but Connect IQ SDK relay callbacks remain unimplemented and fail closed
- `docs/phone-bridge.md`: architecture, wire formats, key-provisioning risks, and limitations of RTT/RSSI versus hardware distance bounding
- `src/crypto.rs`: ChaCha20-Poly1305 AEAD wrapper, `/dev/urandom` nonces, hex key files with permission checks (validated against the RFC 8439 test vector)
- `src/heartbeat.rs`: heartbeat message seal/open with replay, freshness and challenge-binding checks
- `src/att.rs`: minimal ATT PDU codec (MTU exchange, write command, notifications, error responses)
- `src/session.rs`: `SecureSession` probe state machine (outstanding challenge, last seen counter, freshness window)
- L2CAP framing and ATT fixed-channel (CID 0x0004) connect/send/recv in `src/transport.rs`; `HCI Read BD ADDR` support in `src/hci.rs` (local MAC goes into the AAD)
- `BtOps::secure_probe` / `BtOps::local_mac`; `BleDevice::set_secure` with ATT MTU exchange
- `Sample::Unverified` in the monitor: treated like "away" (locks after `--max-missed`)
- CLI: `--key-file`, `--gen-key-file`, `--allow-insecure`, `--heartbeat-window-ms`, `--challenge-timeout-ms`, `--challenge-handle`, `--heartbeat-handle`
- `docs/secure-heartbeat.md`: wire protocol, verification rules, key provisioning, Forerunner 235 guidance, Linux notes, residual risks
- Tests: RFC 8439 vector, heartbeat forgery/replay/stale/challenge-mismatch cases, session probe tests, run-loop tests proving garbage/no-response/replayed heartbeats never unlock, CLI tests for the new options and secure-by-default enforcement

### Changed
- `cargo test`: 140 unit + 7 integration tests pass (incl. bridge mode over a local TCP mock, relay-RTT rejection, and mode-1 forgery/replay/stale tests)
- README security section rewritten; systemd unit now uses `--key-file` and drops root in favor of `CAP_NET_ADMIN`
- Dependency added: `chacha20poly1305` 0.10 (pure Rust, audited RustCrypto crate; still no system libraries such as libbluetooth)

## [0.1.1] - 2026-11-12

### Added
- Comprehensive unit test suite across all core modules (`mac`, `distance`, `sampler`, `clock`, `commands`, `monitor`, `hci`, `cli`) — 74 tests
- Integration tests for the compiled binary (`tests/cli_integration.rs`) covering `--help`, missing args, invalid MAC, and unknown options
- `CHANGELOG.md` following Keep a Changelog format

### Changed
- Derived `Debug` and `PartialEq` on `cli::RunArgs` and `cli::Parsed` to support test assertions
- README testing section clarified

### Removed
- Stale `cargo-xtask` wrapper script (superseded by the `.cargo/config.toml` alias, use `cargo xtask`)
- Unused `src/bnnb_main.rs` (referenced the old `bnnb` crate name)

## [0.1.0] - 2026-11-11

### Added
- Initial Rust port of the original Python `bluetooth-near.py` (BNNB)
- Zero-dependency raw HCI socket communication (no libbluetooth)
- L2CAP connect with RSSI and TX power reading
- Log-distance path-loss model with polynomial accuracy curves
- 7-sample averaging for stable distance measurements
- Edge-triggered lock/unlock after configurable missed polls
- Configurable CLI with dry-run, once, and max-iterations modes
- Clean exit codes (0/1/2/3)
- xtask build pattern with `cargo xtask clean`
- Beerware license
