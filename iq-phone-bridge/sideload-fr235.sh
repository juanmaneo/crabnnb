#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
WORK_DIR="$REPO_ROOT/.hardware-test"
SOURCE_DIR="$WORK_DIR/source-copy"
KEYED_DIR="$REPO_ROOT/target/iq-phone-bridge/keyed-project"
OUTPUT_DIR="$REPO_ROOT/target/iq-phone-bridge/sideload-output"
PAIRING_KEY_FILE="${CRABNNB_KEY_FILE:-$HOME/.Garmin/crabnnb-pairing-key.hex}"
DEVELOPER_KEY="${CIQ_DEVELOPER_KEY:-$HOME/.Garmin/ConnectIQ/keys/crabnnb-developer-4096.der}"
MONKEYC="${CIQ_MONKEYC:-$HOME/.Garmin/ConnectIQ/Sdks/connectiq-sdk-lin-8.1.1-2025-03-27-66dae750f/bin/monkeyc}"
WATCH_MOUNT="${GARMIN_MOUNT:-/run/media/$USER/GARMIN}"

if ! mountpoint -q "$WATCH_MOUNT"; then
    echo "Garmin watch is not mounted at $WATCH_MOUNT" >&2
    exit 1
fi
if [[ ! -d "$WATCH_MOUNT/GARMIN/APPS" ]]; then
    echo "No GARMIN/APPS directory found under $WATCH_MOUNT" >&2
    exit 1
fi
for file in "$PAIRING_KEY_FILE" "$DEVELOPER_KEY" "$MONKEYC"; do
    if [[ ! -f "$file" ]]; then
        echo "Required build file is missing: $file" >&2
        exit 1
    fi
done
if [[ ! -x "$MONKEYC" ]]; then
    echo "Connect IQ compiler is not executable: $MONKEYC" >&2
    exit 1
fi

cleanup() {
    rm -rf "$KEYED_DIR" "$OUTPUT_DIR"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

mkdir -p "$WORK_DIR"
rm -rf "$SOURCE_DIR" "$KEYED_DIR" "$OUTPUT_DIR"
mkdir -p "$SOURCE_DIR" "$KEYED_DIR" "$OUTPUT_DIR"
cp -a "$SCRIPT_DIR/." "$SOURCE_DIR/"
cp -a "$SOURCE_DIR/." "$KEYED_DIR/"

python3 - "$KEYED_DIR/resources/strings.xml" "$PAIRING_KEY_FILE" <<'PY'
from pathlib import Path
import sys

resource = Path(sys.argv[1])
key_path = Path(sys.argv[2])
key = key_path.read_text(encoding="ascii").strip()
if len(key) != 64 or any(char not in "0123456789abcdefABCDEF" for char in key):
    raise SystemExit("Pairing key must contain exactly 64 hexadecimal characters")
text = resource.read_text(encoding="utf-8")
empty = '<string id="PairingKeyEmbedded"></string>'
if text.count(empty) != 1:
    raise SystemExit("Expected exactly one empty PairingKeyEmbedded resource")
resource.write_text(
    text.replace(empty, f'<string id="PairingKeyEmbedded">{key}</string>'),
    encoding="utf-8",
)
PY

(
    cd "$KEYED_DIR"
    "$MONKEYC" -f monkey.jungle -o "$OUTPUT_DIR/PhoneBridge.prg" \
        -d fr235 -y "$DEVELOPER_KEY"
)

cp "$OUTPUT_DIR/PhoneBridge.prg" "$WATCH_MOUNT/GARMIN/APPS/PHONEBRI.PRG"
sync
cmp "$OUTPUT_DIR/PhoneBridge.prg" "$WATCH_MOUNT/GARMIN/APPS/PHONEBRI.PRG"
echo "FR235 sideload verified; temporary keyed source and PRG will now be removed."
