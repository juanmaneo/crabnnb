#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd -- "$1" && pwd)"
SDK_ROOT="${CIQ_SDK_ROOT:-$HOME/.Garmin/ConnectIQ/Sdks}"
if [[ -n "${CIQ_SDK_PATH:-}" ]]; then
    SDK="$CIQ_SDK_PATH"
else
    SDK="$(find "$SDK_ROOT" -maxdepth 1 -type d -name 'connectiq-sdk-lin-*' -print | sort -V | tail -n 1)"
fi
if [[ -z "$SDK" || ! -x "$SDK/bin/monkeyc" ]]; then
    echo "Could not find Connect IQ SDK under $SDK_ROOT (set CIQ_SDK_PATH to override)." >&2
    exit 1
fi
for tool in openssl; do
    command -v "$tool" >/dev/null 2>&1 || { echo "Missing required container tool: $tool" >&2; exit 1; }
done

WORK_DIR="$(mktemp -d)"
SIM_PID=""
XVFB_PID=""
COLLECTOR_PID=""
MONKEYDO_PID=""
TEST_PORT="${CIQ_TEST_PORT:-18765}"
cleanup() {
    [[ -z "$MONKEYDO_PID" ]] || kill "$MONKEYDO_PID" 2>/dev/null || true
    [[ -z "$SIM_PID" ]] || kill "$SIM_PID" 2>/dev/null || true
    [[ -z "$XVFB_PID" ]] || kill "$XVFB_PID" 2>/dev/null || true
    [[ -z "$COLLECTOR_PID" ]] || kill "$COLLECTOR_PID" 2>/dev/null || true
    wait "$MONKEYDO_PID" 2>/dev/null || true
    wait "$SIM_PID" 2>/dev/null || true
    wait "$XVFB_PID" 2>/dev/null || true
    wait "$COLLECTOR_PID" 2>/dev/null || true
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

openssl genrsa -out "$WORK_DIR/test-key.pem" 2048 >/dev/null 2>&1
openssl pkcs8 -topk8 -nocrypt -in "$WORK_DIR/test-key.pem" -outform DER -out "$WORK_DIR/test-key.der"
python3 "$SCRIPT_DIR/collector.py" "$TEST_PORT" "$WORK_DIR/result.txt" >"$WORK_DIR/collector.log" 2>&1 &
COLLECTOR_PID=$!
for attempt in $(seq 1 30); do
    if ! kill -0 "$COLLECTOR_PID" 2>/dev/null; then
        cat "$WORK_DIR/collector.log" >&2
        echo "HTTP result collector exited during startup" >&2
        exit 1
    fi
    if python3 -c 'import socket, sys; s=socket.socket(); s.settimeout(0.2); sys.exit(0 if s.connect_ex(("127.0.0.1", int(sys.argv[1]))) == 0 else 1)' "$TEST_PORT"; then break; fi
    sleep 1
done
if ! python3 -c 'import socket, sys; s=socket.socket(); s.settimeout(0.2); sys.exit(0 if s.connect_ex(("127.0.0.1", int(sys.argv[1]))) == 0 else 1)' "$TEST_PORT"; then
    cat "$WORK_DIR/collector.log" >&2
    echo "HTTP result collector did not become ready on port $TEST_PORT" >&2
    exit 1
fi
"$SDK/bin/monkeyc" -f "$PROJECT_DIR/monkey.jungle" -d fr235 -y "$WORK_DIR/test-key.der" -l 2 -w -o "$WORK_DIR/bridge-test.prg"

command -v Xvfb >/dev/null 2>&1 || { echo "Xvfb is required in the SDK container" >&2; exit 1; }
DISPLAY_NUM="${CIQ_DISPLAY:-:99}"
Xvfb "$DISPLAY_NUM" -screen 0 1280x800x24 -nolisten tcp >"$WORK_DIR/xvfb.log" 2>&1 &
XVFB_PID=$!
export DISPLAY="$DISPLAY_NUM"
XVFB_READY=0
for attempt in $(seq 1 30); do
    if ! kill -0 "$XVFB_PID" 2>/dev/null; then
        cat "$WORK_DIR/xvfb.log" >&2
        echo "Xvfb exited before becoming ready" >&2
        exit 1
    fi
    if [[ -S "/tmp/.X11-unix/X${DISPLAY_NUM#:}" ]]; then XVFB_READY=1; break; fi
    sleep 1
done
if [[ "$XVFB_READY" -ne 1 ]]; then
    cat "$WORK_DIR/xvfb.log" >&2
    echo "Xvfb did not become ready" >&2
    exit 1
fi

"$SDK/bin/simulator" -d fr235 >"$WORK_DIR/simulator.log" 2>&1 &
SIM_PID=$!
sleep 8
if ! kill -0 "$SIM_PID" 2>/dev/null; then
    cat "$WORK_DIR/simulator.log" >&2
    echo "Connect IQ simulator exited during startup" >&2
    exit 1
fi
"$SDK/bin/monkeydo" "$WORK_DIR/bridge-test.prg" fr235 >"$WORK_DIR/monkeydo.log" 2>&1 &
MONKEYDO_PID=$!
TEST_STATUS=0
for attempt in $(seq 1 "${CIQ_TEST_TIMEOUT:-120}"); do
    if [[ -s "$WORK_DIR/result.txt" ]]; then break; fi
    if ! kill -0 "$MONKEYDO_PID" 2>/dev/null; then
        wait "$MONKEYDO_PID" || TEST_STATUS=$?
        break
    fi
    sleep 1
done
if [[ ! -s "$WORK_DIR/result.txt" && "$TEST_STATUS" -eq 0 ]]; then
    TEST_STATUS=124
fi
kill "$SIM_PID" 2>/dev/null || true
wait "$SIM_PID" 2>/dev/null || true
SIM_PID=""
if kill -0 "$MONKEYDO_PID" 2>/dev/null; then
    sleep 2
    kill "$MONKEYDO_PID" 2>/dev/null || true
fi
wait "$MONKEYDO_PID" 2>/dev/null || true
cat "$WORK_DIR/monkeydo.log"
if [[ -s "$WORK_DIR/result.txt" ]]; then cat "$WORK_DIR/result.txt"; fi
if [[ "$TEST_STATUS" -ne 0 ]]; then
    cat "$WORK_DIR/collector.log" >&2
    cat "$WORK_DIR/simulator.log" >&2
    if [[ "$TEST_STATUS" -eq 124 ]]; then
        echo "No Monkey C result was received within ${CIQ_TEST_TIMEOUT:-120}s; the simulator app did not report to the local collector." >&2
    else
        echo "monkeydo exited with code $TEST_STATUS before reporting test results" >&2
    fi
    exit "$TEST_STATUS"
fi
if ! grep -q '^ALL_CRYPTO_TESTS_PASSED' "$WORK_DIR/result.txt"; then
    cat "$WORK_DIR/collector.log" >&2
    cat "$WORK_DIR/simulator.log" >&2
    echo "One or more Monkey C simulator crypto checks failed" >&2
    exit 1
fi
printf 'Connect IQ simulator crypto tests passed using %s\n' "$(basename "$SDK")"
