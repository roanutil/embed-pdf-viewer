---
'@cloudpdf/engine': minor
---

The cloud engine implements `page(pon).annotations.flatten` over `POST …/annotations/pages/{pon}/items/flatten` (content and annotation planes patched in place, `annotations.flattened` published) and `page(pon).annotations.exportAppearance` over `POST …/items/appearance` (PDF bytes; a read).
