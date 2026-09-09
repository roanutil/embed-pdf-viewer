---
'@cloudpdf/engine': minor
---

The cloud engine implements `pages.setName` and `pages.removeName` over `POST …/pages/names` and `POST …/pages/names/delete`. Both advance only `docVersion` and `layoutVersion`, so the cached manifest is patched in place, per-page render/text/annotation leaves stay valid, and the result carries the fresh layout including `namedPages`.
