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

`<arm>-r<round>.json` are the 28 files behind the wasm tables in the same file,
and `node bench/compare.mjs` reproduces those tables from them exactly.

They were missing for a while. `bench/run.mjs` used to clear this whole
directory at the start of every run, so the first `--native` run deleted the
wasm set; commit `a31c1123` re-ran wasm and committed the replacements. A run
now clears only the files it is about to write, and `--out <dir>` keeps an
exploratory run out of this directory altogether, which is what
`bench/run.test.mjs` uses.

All 56 files are current with the sources beside them. They were re-run for
commit `1566b1fb`, which taught `rs_read_page_geometry` and
`cc_read_page_geometry` to report overflow as a negative count and gave
`geometryCoarse` and `searchCoarse` one shared guard against reading one, so the
`wasm bytes` row moved with them: a fresh `bash build/link-wasm.sh` links arm c
at 4,682,622 bytes and arm d at 4,685,996, which is what the published table
says.
