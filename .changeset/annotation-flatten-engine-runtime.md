---
'@embedpdf/engine-runtime': minor
---

`EPDFPage_FlattenAnnotations` flattens a chosen set of a page's annotations with a per-entry status (applied, skipped, not-on-page), replacing the single-annotation `EPDFAnnot_Flatten`. `EPDFPage_ExportAnnotationsAsDocument` flattens a set's appearances into a new single-page document sized to their union rect, replacing `EPDFAnnot_ExportAppearanceAsDocument` and `EPDFAnnot_ExportMultipleAppearancesAsDocument`, which mishandled rotated appearances and the rect fit. Whole-page flatten, selective flatten, and export now share one candidate plan and one placement writer (ISO 32000-2 12.5.5 fit, `/Matrix` honored, no content re-parsing); resources shared between exported appearances are cloned once.
