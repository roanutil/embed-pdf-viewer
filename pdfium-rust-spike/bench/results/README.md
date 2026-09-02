# Raw measurement data

One JSON per arm per round, written by `bench/node.mjs` and aggregated by
`bench/compare.mjs`. Each file carries its own environment stamp (Node version,
OS, CPU, core count, emsdk and rustc versions, loadavg), the fixture's page
index and glyph count, the arm order that round used, and the harness sink
value, because absolute numbers here are machine-specific and a results file
without that header is a trap for whoever reads it later.

`native-<arm>-r<round>.json` are the 28 files behind the `## Native` tables in
`../../RESULTS.md`. `node bench/compare.mjs --native` reproduces those tables
from them exactly.

The wasm equivalents, which would be named `<arm>-r<round>.json`, are missing.
`bench/run.mjs` used to clear this whole directory at the start of every run, so
the first `--native` run deleted the wasm set, and the wasm tables in
`../../RESULTS.md` have no raw data behind them. That bug is fixed: a run now
clears only the files it is about to write. Regenerating the wasm set takes
`bash build/link-wasm.sh && node bench/run.mjs --rounds 7`, but the samples will
be fresh ones rather than the samples behind the published wasm tables.
