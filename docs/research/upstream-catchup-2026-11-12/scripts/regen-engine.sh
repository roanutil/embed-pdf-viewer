#!/usr/bin/env bash
# usage: regen-engine.sh <checkout>  regenerate every engine-conformance vector group
set -uo pipefail
cd "$1/tooling/engine-conformance" || exit 2
export PATH="/workspaces/bin:$PATH"
for s in $(node -p "Object.keys(require('./package.json').scripts).filter(s=>s+':check' in require('./package.json').scripts && s!=='operations').join(' ')"); do
  if pnpm -s run "$s" > "/tmp/regen.$s.log" 2>&1; then echo "$s ok"; else echo "$s FAIL: $(grep -vE '^\s+at |WARN|^\s*$' /tmp/regen.$s.log | tail -1 | cut -c1-200)"; fi
done
