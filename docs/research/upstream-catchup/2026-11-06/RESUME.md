# Active implementation resume

Read [IMPLEMENTATION.md](IMPLEMENTATION.md) first. User authorized catch-up implementation; it is now in progress. Fixed targets and baselines remain in pinned-targets.json.

1. Root integration: container embed-pdf, /workspaces/catchup-2026-11-06, branch research/catchup-2026-11-06. Main planning checkout /workspaces/embed-pdf-viewer is preserved.
2. Runtime integration: /workspaces/catchup-runtime-2026-11-06, branch catchup/2026-11-06-runtime. Job1 initial Linux ARM64/WASM inputs are complete at 1e214deb2. Read committed receipts before considering another build.
3. Both existing platform packages and TS/Rust conformance vectors must stay current; expand scenarios for new behavior. Read PARITY-AND-PLATFORMS.md.
4. Commit bounded work, decisions and actual test results. Push only to user forks. Preserve both starting histories and do not advance research refs before final acceptance.
5. Next: Job3 remaining contracts and ten generator typecheck diagnostics, then plugin/binding/package acceptance. The final combined Rust browser WASM is not built. See IMPLEMENTATION.md; historical planning evidence is at commit4289d3f86 and historical/.

Next action (under two minutes): inspect the ledger and git status in the integration worktree before resuming.
