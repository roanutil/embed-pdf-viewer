---
'@cloudpdf/server': minor
---

Add `POST /v1/docs/{docId}/layers/{layerName}/annotations/pages/{pon}/items/flatten` (flatten a chosen set of the page's annotations; gated like page flatten and persisted the same way — one page's content and annotation versions advance) and `POST …/items/appearance` (the chosen annotations' appearances as one single-page PDF; gated by `doc.download`, no-store).
