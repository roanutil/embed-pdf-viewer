---
'@embedpdf/engine': minor
---

The local engine implements `pages.setName` and `pages.removeName`: register, rename, or remove a `/Names /Pages` entry as a page-structure mutation gated by `doc.pages.assemble`, with the fresh layout returned and a `pages.named` event published. `pages.list()` includes `namedPages`.
