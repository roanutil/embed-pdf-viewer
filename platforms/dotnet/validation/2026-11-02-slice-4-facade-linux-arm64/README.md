# Slice 4 facade validation

The record pins clean implementation commit `849d3246ad0d1b23abd69561d4361ebafd8869c0`. This commit adds evidence only. Commands ran in the required Linux arm64 image with a private Cargo target volume and the unchanged shared PDFium mounted read-only at `/opt/pdfium`. The binary artifact paths are relative to the tested source worktree. Logs normalize only the container source/target prefixes.

`dotnet test` passes 31 tests with zero skips; `epdf-capi` passes 64 tests. The broad serial engine run fails at the unchanged held-layer-page SIGSEGV tracked by issue #16 after 743 completed-target tests and 8 document-session tests. No later engine target is inferred to pass. Packaging, relocation and browser parity remain separate acceptance work.
