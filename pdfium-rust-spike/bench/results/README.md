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

All 56 files predate the fixes for the code review's findings 3, 9 and 10
(`rs_search_page` overflow reporting, f64 promotion in the rotation branch,
full-width text-object keys). None of the three moves a published timing cell:
the rotation branch never runs on the timing fixture (`report.pdf` page 4 has 0
non-upright glyphs of 8,106), and the other two add one integer comparison per
call. The `wasm bytes` row is the exception, since the new code is compiled in:
a fresh `bash build/link-wasm.sh` now links arm c at 4,682,602 bytes and arm d
at 4,685,976, against the 4,681,488 and 4,684,877 in the published table.
