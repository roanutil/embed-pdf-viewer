---
'@cloudpdf/server': minor
---

Add `POST /v1/docs/{docId}/layers/{layerName}/pages/names` and `POST …/pages/names/delete` to register, rename, or remove a `/Names /Pages` entry on a layer. Both are page-structure mutations gated by `doc.pages.assemble`: the worker writes a new layer artifact and the doc and layout versions advance, exactly like a page move, with no new resource, version, or cache scope. `/layout` responses now include `namedPages`.
