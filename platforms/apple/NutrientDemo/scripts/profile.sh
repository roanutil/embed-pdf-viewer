#!/usr/bin/env bash
# Record one Instruments template attached to the demo app while
# NutrientDemoUITests drives it. Build first with `xcodebuild build-for-testing
# ... -derivedDataPath .build/dd` (see platforms/apple/README.md).
#
# usage: scripts/profile.sh "<template>" <out.trace> [simulator-id]
set -euo pipefail

TEMPLATE="$1"
OUT="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
DEMO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DD="${DERIVED_DATA:-$DEMO/.build/dd}"
SIM="${3:-$(xcrun simctl list devices available -j |
  /usr/bin/python3 -c 'import json,sys; d=json.load(sys.stdin)["devices"]; print(next(x["udid"] for r in d.values() for x in r if x["name"].startswith("iPhone")))')}"

xcrun simctl boot "$SIM" 2>/dev/null || true
xcrun simctl terminate "$SIM" com.embedpdf.nutrientdemo 2>/dev/null || true
# Nutrient keeps a disk cache of rendered pages across launches; start cold.
xcrun simctl uninstall "$SIM" com.embedpdf.nutrientdemo 2>/dev/null || true
rm -rf "$OUT"

cd "$DEMO"
TEST_RUNNER_EMBEDPDF_PROFILE_DELAY=10 xcodebuild test-without-building \
  -workspace NutrientDemo.xcworkspace -scheme NutrientDemo \
  -destination "platform=iOS Simulator,id=$SIM" -derivedDataPath "$DD" \
  -only-testing:NutrientDemoUITests > "$OUT.uitest.log" 2>&1 &
TEST=$!

PID=""
for _ in $(seq 1 120); do
  PID=$(pgrep -f 'NutrientDemo.app/NutrientDemo$' | head -1 || true)
  [ -n "$PID" ] && break
  sleep 0.5
done
if [ -z "$PID" ]; then
  echo "the app never started; see $OUT.uitest.log" >&2
  exit 1
fi

xcrun xctrace record --template "$TEMPLATE" --device "$SIM" --attach "$PID" \
  --time-limit 75s --output "$OUT"

wait "$TEST" || echo "the UI test failed; see $OUT.uitest.log" >&2
