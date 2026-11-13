#!/usr/bin/env bash
# usage: platforms.sh <checkout> <label> [runtime-manifest]
# The platforms/ checks on Linux arm64, each step's exit code in <out>/summary.txt:
# .NET facade tests, parity self-test, packaging unit tests, pack + validate linux-arm64 (with no
# LD_LIBRARY_PATH, so the relocation check sees only what the package carries);
# Kotlin command-context and checksum tests on a desktop JVM; Swift on Linux (swift-linux.sh).
set -uo pipefail
C="$1"; LABEL="$2"; MANIFEST="${3:-$C/packages/engine/runtime/engine-runtime-build.json}"
OUT=/workspaces/logs/CU-C7-plat-$LABEL; rm -rf "$OUT"; mkdir -p "$OUT"; R=$OUT/summary.txt; : > $R
T=/workspaces/tools
export DOTNET_ROOT=$T/dotnet JAVA_HOME=$T/jdk DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1
export PATH=$T/dotnet:$T/jdk/bin:$T/kotlinc/bin:/workspaces/bin:$PATH CI=true
export CARGO_TARGET_DIR=$C/target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
LIBPDFIUM=$C/packages/engine/runtime/build/libpdfium
export PDF_RUNTIME_LIB_DIR=$LIBPDFIUM LD_LIBRARY_PATH=$LIBPDFIUM/linux-arm64/lib
export EPDF_IMAGE_ID=sha256:30b97dec3bd8d007b9197ab8fc0027f3b62ea3bd25390a5f516b4cbab085df84 EPDF_HOST_OS=macOS
cd "$C" || exit 2
echo "checkout $(git rev-parse HEAD) manifest $MANIFEST start $(date -u +%FT%TZ)" >> $R
cargo build --locked -p epdf-capi > $OUT/capi-build.log 2>&1; echo "capi build exit=$?" >> $R
EPDF_NATIVE_DIR=$CARGO_TARGET_DIR/debug dotnet test platforms/dotnet/EmbedPDF.Tests/EmbedPDF.Tests.csproj > $OUT/dotnet-test.log 2>&1; echo "dotnet test exit=$?" >> $R
grep -E "^(Passed|Failed)!|Total tests|Passed:|Failed:" $OUT/dotnet-test.log | tail -3 >> $R
(cd platforms/dotnet && dotnet run --project EmbedPDF.Parity -- self-test) > $OUT/parity.log 2>&1; echo "parity self-test exit=$? passed=$(grep -c '"status":"passed"' $OUT/parity.log)" >> $R
python3 -m unittest discover -s platforms/dotnet/packaging -v > $OUT/packaging-unittest.log 2>&1; echo "packaging unittest exit=$? $(tail -1 $OUT/packaging-unittest.log)" >> $R
cargo build --locked -p epdf-capi --release --target aarch64-unknown-linux-gnu > $OUT/capi-release.log 2>&1; echo "capi release build exit=$?" >> $R
env -u LD_LIBRARY_PATH bash platforms/dotnet/pack.sh --rid linux-arm64 --out $OUT/pack --runtime-manifest "$MANIFEST" \
  --runtime-archive /workspaces/pdfium/archives/libembedpdf-runtime-linux-arm64-aa59ce554.tar.gz > $OUT/pack.log 2>&1; echo "pack exit=$?" >> $R
env -u LD_LIBRARY_PATH bash platforms/dotnet/validate.sh --rid linux-arm64 --runtime-manifest "$MANIFEST" --record $OUT/validate-record.json \
  --package $OUT/pack/EmbedPDF.Engine.0.0.0-local.nupkg --manifest $OUT/pack/package-manifest.linux-arm64.json > $OUT/validate.log 2>&1; echo "validate exit=$? $(tail -1 $OUT/validate.log)" >> $R
JNA_JAR=$T/jars/jna-5.14.0.jar COROUTINES_JAR=$T/jars/kotlinx-coroutines-core-jvm-1.10.2.jar KOTLINC=$T/kotlinc/bin/kotlinc \
  LD_LIBRARY_PATH=$LIBPDFIUM/linux-arm64/lib:$CARGO_TARGET_DIR/debug bash platforms/android/test-command-context.sh > $OUT/kotlin.log 2>&1; echo "kotlin command-context exit=$?" >> $R
bash /workspaces/swift-linux/swift-linux.sh "$C" $OUT/swift > $OUT/swift-driver.log 2>&1; echo "swift-linux exit=$? $(cat $OUT/swift/build.exit $OUT/swift/test.exit 2>/dev/null | tr '\n' ' ')" >> $R
grep -E "Test run with|Executed [0-9]+ tests" $OUT/swift/test.log 2>/dev/null | tail -3 >> $R
echo "end $(date -u +%FT%TZ)" >> $R; echo PLATDONE >> $R
