#!/usr/bin/env bash
# usage: suite.sh <checkout-dir> <out-dir> [parts...]  parts: cargo engconf plugconf ts (default all)
set -uo pipefail
DIR="$1"; OUT="$2"; shift 2
PARTS="${*:-cargo engconf plugconf ts}"
mkdir -p "$OUT"; cd "$DIR" || exit 2
export CARGO_TARGET_DIR="$DIR/target" CI=true
export PDF_RUNTIME_LIB_DIR="$DIR/packages/engine/runtime/build/libpdfium"
export LD_LIBRARY_PATH="$DIR/packages/engine/runtime/build/libpdfium/linux-$(node -p process.arch)/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH" > "$OUT/env.txt"
git -C "$DIR" rev-parse HEAD >> "$OUT/env.txt"
for p in $PARTS; do case $p in
cargo)
  echo "=== $(date +%T) cargo"
  CARGO_BUILD_JOBS=12 cargo test --workspace --no-fail-fast > "$OUT/cargo.log" 2>&1; echo "exit=$?" > "$OUT/cargo.exit"
  grep -E "^test .* \.\.\. FAILED$" "$OUT/cargo.log" | sed -E 's/^test (.*) \.\.\. FAILED$/\1/' > "$OUT/cargo.failed.raw"
  # attach binary name: track "Running ..." headers
  awk '/^     Running /{bin=$2} /^test .* \.\.\. FAILED$/{sub(/^test /,"");sub(/ \.\.\. FAILED$/,""); print bin" :: "$0}' "$OUT/cargo.log" | sort > "$OUT/cargo.failed"
  grep -E "^error(\[|:)" "$OUT/cargo.log" | sort | uniq > "$OUT/cargo.errors"
  grep -E "^test result:" "$OUT/cargo.log" | awk '{p+=$4; f+=$6; i+=$8} END {print "passed="p" failed="f" ignored="i}' > "$OUT/cargo.summary"
  ;;
engconf)
  echo "=== $(date +%T) engine-conformance"
  : > "$OUT/engconf.failed"
  for s in $(node -p "Object.keys(require('./tooling/engine-conformance/package.json').scripts).filter(s=>s.endsWith(':check')).join(' ')"); do
    if (cd tooling/engine-conformance && timeout 1800 pnpm run -s "$s") > "$OUT/engconf.$s.log" 2>&1; then echo "$s ok" >> "$OUT/engconf.all"; else echo "$s" >> "$OUT/engconf.failed"; echo "$s FAIL" >> "$OUT/engconf.all"; fi
  done
  ;;
plugconf)
  echo "=== $(date +%T) plugin-conformance"
  : > "$OUT/plugconf.failed"
  for s in vectors:check coverage:check test:harness typecheck; do
    if (cd tooling/plugin-conformance && timeout 3600 pnpm run -s "$s") > "$OUT/plugconf.$s.log" 2>&1; then echo "$s ok" >> "$OUT/plugconf.all"; else echo "$s" >> "$OUT/plugconf.failed"; echo "$s FAIL" >> "$OUT/plugconf.all"; fi
  done
  ;;
ts)
  echo "=== $(date +%T) ts"
  pnpm exec turbo run test --continue --concurrency=6 --filter='./packages/**' > "$OUT/ts.log" 2>&1; echo "exit=$?" > "$OUT/ts.exit"
  grep -E "#test: command .* exited \([0-9]+\)|ERROR: command finished with error|Failed:" "$OUT/ts.log" | sort -u > "$OUT/ts.failed"
  grep -oE "^[^ ]+:test: (FAIL|✗|×) .*" "$OUT/ts.log" | sort -u > "$OUT/ts.failedtests"
  ;;
esac; done
echo "=== $(date +%T) SUITE DONE"
