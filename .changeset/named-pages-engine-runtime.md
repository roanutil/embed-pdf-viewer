---
'@embedpdf/engine-runtime': minor
---

New named-page functions: `EPDFDoc_GetNamedPageCount`, `EPDFDoc_GetNamedPageAt` (key as UTF-16 plus the value's object number and kind: page, template, or dangling), `EPDFDoc_SetNamedPage` (create or replace, pages in the page tree only), `EPDFDoc_RemoveNamedPage`, and `EPDFDoc_RemoveNamedPagesForPage`. `EPDFDoc_DeletePageByObjectNumber` now removes the `/Names /Pages` registrations of the page it deletes. The name-tree index search reports a pair whose value is a missing object (with a null value) instead of hiding it and desynchronizing later indices.

Annotation `/Name` is text: `EPDFAnnot_SetName` takes any name, writes a name object (escaping applied by the serializer), and never touches `/AP`; `EPDFAnnot_GetName` fills a text buffer. The `FPDF_ANNOT_NAME` enum, its subtype validation, and the sentinel that removed `/Name` together with `/AP` are removed — remove `/Name` with `EPDFAnnot_RemoveKey(annot, "Name")`.
