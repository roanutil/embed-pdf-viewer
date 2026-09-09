---
'@embedpdf/engine': minor
---

The local engine implements `page(pon).annotations.flatten` (gated like `pages.flatten`, publishes `annotations.flattened`) and `page(pon).annotations.exportAppearance` (gated by `doc.download` like `pages.extract`).
