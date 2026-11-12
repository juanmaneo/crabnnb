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

CRABNNB monitors Bluetooth device proximity and can lock/unlock your session based on device presence, making it perfect for automatically securing your computer when you walk away.

## Minimum Supported Rust Version (MSRV)

This project requires Rust 1.70.0 or later.

## Features

- Zero-dependency Rust implementation
- Raw HCI socket communication (no libbluetooth)
- L2CAP connect with RSSI and TX power reading
- Log-distance path-loss model with polynomial accuracy curves
- Sample averaging for stable distance measurements
- Edge-triggered locking after configurable missed polls
- Configurable commands for lock/unlock actions
- Dry-run mode for safe testing
- Clean exit codes (0/1/2/3)
- Systemd service integration

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
```

## Examples

```bash
# Basic usage with default settings
cargo run -- AA:BB:CC:DD:EE:FF

# Default KDE Plasma commands
cargo run -- DEVICE_MAC_ADDRESS --lock-cmd "loginctl lock-session" --unlock-cmd "loginctl unlock-session"

# Dry run mode for testing (no commands executed)
cargo run -- AA:BB:CC:DD:EE:FF --dry-run

# Run only once and exit (useful for testing)
cargo run -- AA:BB:CC:DD:EE:FF --once

# Discover nearby Bluetooth devices
cargo run -- --once --dry-run
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
  --max-distance-cm 150 \
  --rssi-at-1m -65 \
  --samples 5 \
  --sample-sleep-ms 100 \
  --lock-cmd "loginctl lock-session" \
  --unlock-cmd "loginctl unlock-session"
Restart=always
RestartSec=30
Environment="RUST_LOG=info"

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
- 1: General error
- 2: Bluetooth error
- 3: Command execution error

## Security Considerations

1. The Bluetooth MAC address is stored in configuration files
2. Lock/unlock commands run with your user privileges
3. Ensure your system's Bluetooth stack is up-to-date
4. Consider the privacy implications of Bluetooth device tracking