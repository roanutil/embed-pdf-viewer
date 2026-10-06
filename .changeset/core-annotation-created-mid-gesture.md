---
'@embedpdf/core-annotation': patch
---

A move, resize, caption or leader drag that starts before the engine confirms a new annotation now carries over to the annotation's permanent id when the confirmation arrives. Previously the next pointer move threw. A drag whose annotation has disappeared now ends the gesture instead of throwing.
