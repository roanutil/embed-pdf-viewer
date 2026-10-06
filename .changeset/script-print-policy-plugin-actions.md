---
'@embedpdf/plugin-actions': patch
---

A script's `print()` now obeys the `print` policy row for the dispatch's origin. Under the default policy a lifecycle or hover script can no longer open the print dialog, whatever UI adapter is installed.
