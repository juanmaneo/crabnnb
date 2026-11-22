#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
CONTAINER="${CIQ_CONTAINER:-ciq-sdk}"
WORK_DIR="$(mktemp -d "$PROJECT_ROOT/.iq-sim.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT
TEST_PORT="${CIQ_TEST_PORT:-18765}"

if ! command -v podman >/dev/null 2>&1; then
    echo "podman is required to run the Connect IQ simulator harness" >&2
    exit 1
fi
if ! podman container exists "$CONTAINER"; then
    echo "Podman container '$CONTAINER' does not exist. Set CIQ_CONTAINER or start your CIQ SDK container." >&2
    exit 1
fi

cargo xtask iq-vectors > "$WORK_DIR/IqVectors.mc"
cp -a "$PROJECT_ROOT/iq-phone-bridge" "$WORK_DIR/project"
cp "$SCRIPT_DIR/BridgeCryptoTest.mc" "$WORK_DIR/project/source/BridgeCryptoTest.mc"
cp "$WORK_DIR/IqVectors.mc" "$WORK_DIR/project/source/IqVectors.mc"
sed -i "s/__CIQ_TEST_PORT__/$TEST_PORT/g" "$WORK_DIR/project/source/BridgeCryptoTest.mc"
sed -i 's/id="d72f71b21bf64c49a9ac3c6a670ea1f4"/id="1234567890abcdef1234567890abcdef"/' "$WORK_DIR/project/manifest.xml"
sed -i '/var storedKey =/i\        new BridgeCryptoTest().run();' "$WORK_DIR/project/source/PhoneBridgeApp.mc"

printf 'Running FR235 Monkey C crypto vectors in Podman container %s...\n' "$CONTAINER"
podman exec --user neo \
    -e CIQ_TEST_TIMEOUT="${CIQ_TEST_TIMEOUT:-120}" \
    -e CIQ_TEST_PORT="$TEST_PORT" \
    -e CIQ_SDK_PATH="${CIQ_SDK_PATH:-}" \
    -e CIQ_SDK_ROOT="${CIQ_SDK_ROOT:-}" \
    "$CONTAINER" bash "$SCRIPT_DIR/inner.sh" "$WORK_DIR/project"
