---
'@embedpdf/react': patch
---

`Toolbar` no longer re-renders on every kernel change. It re-renders only when a command it showed in its last render now resolves differently, so a scroll step or a tile landing leaves the toolbar alone. Within a render it resolves each command once instead of once per read; event handlers still resolve fresh. `usePages().pageCount` now reads the document's page count directly instead of listing every page's layout on each store change. The count it returns is unchanged.
