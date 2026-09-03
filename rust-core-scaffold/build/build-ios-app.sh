#!/usr/bin/env bash
# Generates the Xcode project and builds the app for one destination.
#
#   bash build/build-ios-app.sh                          # macOS, works today
#   bash build/build-ios-app.sh 'platform=iOS Simulator,name=iPhone 16'
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESTINATION="${1:-platform=macOS}"

( cd "$ROOT/apps/ios" && xcodegen generate )

xcodebuild \
  -project "$ROOT/apps/ios/EpdfScaffold.xcodeproj" \
  -scheme EpdfScaffold \
  -destination "$DESTINATION" \
  build
