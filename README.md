# CRABNNB - Crab's Remote Access Bluetooth Nearby Notification Blocker

CRABNNB is a pure Rust port of the original Python [bluetooth-near.py](https://github.com/juanmaneo/BNNB) script.

The name CRABNNB is a doubly recursive acronym:
- **First level**: **C**rab's **R**emote **A**ccess **B**luetooth **N**earby **N**otification **B**locker
- **Second level**: **BNNB** = Bluetooth Near Not Blueproximity (the original recursive acronym)

This naming reflects the project's core functionality:
- **Remote Access**: Bluetooth proximity detection
- **Nearby Notification**: Device presence monitoring
- **Blocker**: Automatic session locking for security

## License

This project is licensed under the [Beerware License](LICENSE).

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for release history.

CRABNNB monitors Bluetooth device proximity and can lock/unlock your session based on device presence, making it perfect for automatically securing your computer when you walk away.

## Minimum Supported Rust Version (MSRV)

This project requires Rust 1.70.0 or later.

## Features

- Raw HCI socket communication (no libbluetooth; pure-Rust crates only)
- **Encrypted, authenticated heartbeat (challenge-response) between watch and PC** — secure by default; MAC+RSSI alone is never enough to unlock
- ChaCha20-Poly1305 (RFC 8439) AEAD with replay protection (monotonic counter), freshness window and challenge binding (direct BLE), or AES-256-CBC + HMAC-SHA256 encrypt-then-MAC (phone bridge)
- Challenge round-trip latency limit (`--max-roundtrip-ms`); this is not physical distance bounding and does not stop every relay
- Phone-bridge prototype (`--bridge-addr`) for watches that cannot host GATT; Android/Connect IQ integration is incomplete and untested
- L2CAP/ATT (GATT client) channel with MTU exchange, plus RSSI and TX power reading
- Log-distance path-loss model with polynomial accuracy curves
- Sample averaging for stable distance measurements
- Edge-triggered locking after configurable missed polls
- Configurable commands for lock/unlock actions
- Dry-run mode for safe testing
- Clean exit codes (0/1/2/3)
- Systemd service integration

See [docs/secure-heartbeat.md](docs/secure-heartbeat.md) for the protocol specification and threat model.

## Installation

```bash
# Clone the repository
git clone https://github.com/juanmaneo/crabnnb.git
cd crabnnb

# Build the release version
cargo build --release
```

## Usage

```bash
cargo run -- <btaddr> [OPTIONS]
```

### Options:

```
<btaddr>               Bluetooth device MAC address (required) [format: XX:XX:XX:XX:XX:XX]
--lock-cmd CMD         Command to lock the session (default: "loginctl lock-session")
--unlock-cmd CMD       Command to unlock the session (default: "loginctl unlock-session")
--max-distance-cm F    Maximum distance in centimeters (default: 90)
--max-missed N         Consecutive far/away polls before locking (default: 3)
--scan-period SECS     Delay between polls in seconds (default: 1.0)
--samples N            RSSI samples averaged per poll (default: 7)
--sample-sleep-ms MS   Delay between samples in ms (default: 5)
--max-attempts N       Max sampling attempts per poll (default: 70)
--rssi-at-1m DBM       Measured power at 1 meter (default: -59)
--recv-timeout-ms MS   HCI receive timeout in ms (default: 10000)
--dry-run              Print actions instead of executing commands
--once                 Run a single poll and exit
--max-iterations N     Stop after N polls (0 = unlimited, default: 0)
--quiet                Suppress informational output

Secure heartbeat (required unless --allow-insecure):
--key-file PATH        File with 32-byte key as 64 hex chars (mode 0600)
--gen-key-file PATH    Generate a new random key file and exit
--allow-insecure       Allow legacy MAC+RSSI mode without a key (spoofable)
--heartbeat-window-ms MS  Max accepted heartbeat timestamp age/skew (default: 5000)
--challenge-timeout-ms MS  Wait for the heartbeat reply per poll (default: 2000)
--challenge-handle H   ATT handle of the challenge characteristic (default: 0x0031)
--heartbeat-handle H   ATT handle of the heartbeat characteristic (default: 0x0034)
--max-roundtrip-ms MS  Reject slow challenge responses (latency limit, not distance proof; default 250; 0 disables)
--bridge-addr HOST:PORT  Use the phone-bridge TCP transport instead of direct BLE (requires --key-file)
```

### Phone bridge (for devices that cannot host GATT, e.g. Forerunner 235)

```bash
# PC: connect to the phone companion (e.g. adb forward tcp:8765 tcp:8765)
crabnnb AA:BB:CC:DD:EE:FF --key-file /etc/crabnnb/key.hex --bridge-addr 127.0.0.1:8765
```

The intended watch app is in [`iq-phone-bridge/`](iq-phone-bridge/README.md) and
Android companion prototype in [`phone-bridge-android/`](phone-bridge-android/README.md).
The bridge is not operational yet: Connect IQ callbacks are unfinished, the apps
are not built against their SDKs, and no FR235 hardware test has been done. See
[`docs/phone-bridge.md`](docs/phone-bridge.md) for status and wire format.

### Provisioning the shared key

```bash
# On the Linux PC: create the key (mode 0600, never overwritten)
crabnnb --gen-key-file /etc/crabnnb/key.hex

# Copy the 64 hex chars to the watch side out-of-band (USB/QR/manual entry),
# then run:
crabnnb WATCH_MAC_ADDRESS --key-file /etc/crabnnb/key.hex
```

## Examples

```bash
# Basic secure usage (encrypted heartbeat required to unlock)
cargo run -- AA:BB:CC:DD:EE:FF --key-file /etc/crabnnb/key.hex

# Default KDE Plasma commands
cargo run -- DEVICE_MAC_ADDRESS --key-file /etc/crabnnb/key.hex \
  --lock-cmd "loginctl lock-session" --unlock-cmd "loginctl unlock-session"

# Dry run mode for testing (no commands executed)
cargo run -- AA:BB:CC:DD:EE:FF --key-file /etc/crabnnb/key.hex --dry-run

# Run only once and exit (useful for testing)
cargo run -- AA:BB:CC:DD:EE:FF --key-file /etc/crabnnb/key.hex --once

# Legacy MAC+RSSI mode (spoofable; prints a warning) — testing only
cargo run -- AA:BB:CC:DD:EE:FF --allow-insecure --dry-run
```
## Device Configuration

### 1. Find Your Device's MAC Address

```bash
cargo run -- --once --dry-run
```

This will display discoverable Bluetooth devices. Note your device's MAC address.

### 2. Test Device Connection

```bash
cargo run -- DEVICE_MAC_ADDRESS --once --dry-run
```

### 3. Configuration Parameters

```bash
cargo run -- DEVICE_MAC_ADDRESS \
  --max-distance-cm 150 \
  --rssi-at-1m -65 \
  --samples 5 \
  --sample-sleep-ms 100 \
  --lock-cmd "loginctl lock-session" \
  --unlock-cmd "loginctl unlock-session"
```

### 3. Systemd Service Setup

For automatic operation, create a systemd service:

```bash
mkdir -p ~/.config/systemd/user
cat > ~/.config/systemd/user/crabnnb.service << 'EOF'
[Unit]
Description=CRABNNB Bluetooth Proximity Service
After=network.target bluetooth.target
Documentation=https://github.com/juanmaneo/BNNB

[Service]
Type=simple
ExecStart=/path/to/crabnnb/target/release/crabnnb DEVICE_MAC_ADDRESS \
  --key-file /etc/crabnnb/key.hex \
  --max-distance-cm 150 \
  --rssi-at-1m -65 \
  --samples 5 \
  --sample-sleep-ms 100 \
  --lock-cmd "loginctl lock-session" \
  --unlock-cmd "loginctl unlock-session"
Restart=always
RestartSec=30
Environment="RUST_LOG=info"
# HCI raw sockets need CAP_NET_ADMIN; avoid running as full root
AmbientCapabilities=CAP_NET_ADMIN
CapabilityBoundingSet=CAP_NET_ADMIN
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/etc/crabnnb
PrivateTmp=true

[Install]
WantedBy=default.target
EOF
```

**Important:** Replace `DEVICE_MAC_ADDRESS` with your actual device MAC address and update the executable path as needed.

### 4. Enable and Start the Service

```bash
# Reload systemd
systemctl --user daemon-reload

# Enable the service to start on boot
enable crabnnb.service

# Start the service immediately
systemctl --user start crabnnb.service
```

### 5. Service Management

```bash
# Check service status
systemctl --user status crabnnb.service

# View logs
journalctl --user -u crabnnb.service -f

# Restart the service
systemctl --user restart crabnnb.service

# Stop the service
systemctl --user stop crabnnb.service
```

### Configuration Tips

1. **Battery Considerations:**
   ```bash
   --scan-period 3 \
   --samples 3 \
   --sample-sleep-ms 200
   ```

2. **Calibration:** For accurate distance measurement:
   1. Place your device exactly 1 meter from your computer
   2. Run: `cargo run -- DEVICE_MAC_ADDRESS --once --dry-run`
   3. Note the RSSI value and use it for `--rssi-at-1m`

3. **Troubleshooting:**
   - Ensure Bluetooth is enabled on your device
   - Verify device is discoverable
   - Try adjusting `--rssi-at-1m` (typical range: -50 to -70)
   - Check system Bluetooth functionality

## Build Commands

This project uses the standard Rust xtask pattern for custom build commands:

```bash
# Clean build artifacts and profiling files
cargo xtask clean
```

## Testing

Run the full test suite (unit + integration tests):

```bash
cargo test
```

## Exit Codes

- 0: Success
- 1: General error (including missing `--key-file` in secure mode)
- 2: Bluetooth error
- 3: Device lookup/command execution error

## Security Considerations

1. **Presence now requires a cryptographic heartbeat, not just the MAC.** Every poll the PC writes a fresh 16-byte challenge to the watch over ATT and requires a ChaCha20-Poly1305-authenticated reply (counter + timestamp + challenge echo, AAD = both MACs) within `--heartbeat-window-ms`. A Flipper Zero can spoof the MAC and fake the RSSI, but cannot forge the AEAD tag without the pre-shared key. See [docs/secure-heartbeat.md](docs/secure-heartbeat.md).
2. Unlock additionally requires the RSSI-derived distance to be below `--max-distance-cm` (second factor).
3. The key file must be mode `0600`; crabnnb refuses looser permissions. Provision it out-of-band, never over BLE. Rotate with `--gen-key-file` if it leaks.
4. Lock/unlock commands run with your user privileges; run the service with `CAP_NET_ADMIN` only (see the systemd unit above), not as full root.
5. `--allow-insecure` restores the old MAC+RSSI-only behavior and is intended for testing only — it is spoofable and prints a warning.
6. Relay attacks: `--max-roundtrip-ms` (default 250 ms) is only an application-level latency limit, not a physical distance bound. A relay that responds within the threshold can pass; direct-BLE RSSI is also not secure distance bounding. Use suitable, validated UWB/BLE Channel Sounding hardware if physical distance assurance is required. See [docs/phone-bridge.md](docs/phone-bridge.md#relay-attacks-and-round-trip-latency).
7. Ensure your system's Bluetooth stack is up-to-date; consider the privacy implications of Bluetooth device tracking.

### Garmin Forerunner 235 note

Connect IQ apps cannot host a GATT server (`Toybox.BluetoothLowEnergy` is central-only and unavailable on the FR235), so the FR235 cannot directly run the responder side of this protocol. Options: phone bridge via `Toybox.Communications`, an ANT+ channel to an ANT+ USB stick on the PC, a newer CIQ 3.1+ watch as BLE central against a BlueZ GATT server on the PC, or any BLE peripheral running the reference responder. The crypto/heartbeat modules are transport-agnostic. Details in [docs/secure-heartbeat.md](docs/secure-heartbeat.md).