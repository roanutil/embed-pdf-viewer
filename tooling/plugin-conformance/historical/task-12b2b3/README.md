# Immutable B3 original baseline

BASE `d8034e3f6f5f4a99b1ae785146473f4de71fa262`. This archive was copied before measurement source mutation. All211 original cases are preserved in `original-inputs.json`; all eight exact source files are the `.ts.txt` siblings, with source hashes and counts in `summary.json`. `archive-sha256.json` pins each captured artifact.

`inputs-runtime.json.gz` losslessly preserves the entire original17MB input/outcome/trace capture. Gzip timestamp is zero. Decompress with `gzip -dc inputs-runtime.json.gz`; the uncompressed SHA256 is in the hash manifest. `channels.json` provides readable exact original returned/thrown/promise/settled/callback/report/unmatched channels. `diagnostics.json` preserves the original68 compiler rows. `c-signature-inputs.json` preserves all58 deferred signature inputs and four legacy source hashes. `task11-stamp-drift.json` isolates the sole owned B3 Task11 input drift.

Later slices must compare against these d803 originals rather than recapture later heads. No artifact here is a regenerated vector or a current semantic oracle expectation. Runtime exceptions truncated15 scenario traces;828 observed steps/65 unmatched are lower bounds, not full-case success. Original211 = measurement61 +redaction50 +stamp100. Full fresh inventory95stale/17throwing suites/2 helper-export discovery rows remains deferred; this archive does not declare closure.

Measurement i accounting and current source evidence live separately under `../task-12b2b3-i/`. Keeping the original archive immutable preserves exact later family accounting and C signature inputs.
