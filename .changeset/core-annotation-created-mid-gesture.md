---
'@embedpdf/core-annotation': patch
---

A move, resize, caption or leader drag whose annotation goes mid-gesture (another session deletes it, or the engine refuses its create) now ends with nothing written. Before, the next pointer move or release threw. `computeMoveSnap` skips an id with no record instead of throwing.
