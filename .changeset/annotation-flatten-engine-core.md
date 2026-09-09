---
'@embedpdf/engine-core': minor
---

Selective annotation flatten and appearance export. `page(pon).annotations.flatten(refs, usage?)` is `pages.flatten` for a chosen set: painted annotations are removed from the page, ineligible ones (hidden for the usage, popups, no appearance) stay and report `skipped`, and the result (`AnnotationFlattenResult`) carries that page's new pins plus an `annotations.flattened` event. `page(pon).annotations.exportAppearance(refs)` returns the chosen annotations' normal appearances as one single-page PDF sized to their union rect — vector, positions preserved, the source untouched; all-or-nothing. Both are optional service members; `runAnnotationFlattenConformance` and `runAnnotationAppearanceExportConformance` lock the shared behavior.
