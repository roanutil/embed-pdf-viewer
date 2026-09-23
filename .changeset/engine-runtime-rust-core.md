---
'@embedpdf/engine-runtime': minor
---

Expose a linked Rust engine core through `PdfRuntimeModule.rust`, typed as `PdfRuntimeRustCore`. The field is present only in runtime payloads built with the core compiled in, and `undefined` otherwise, so callers branch on presence rather than on a build flag. Published payloads do not include it yet and behave exactly as before.
