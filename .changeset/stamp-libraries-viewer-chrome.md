---
'@embedpdf/viewer-chrome': minor
---

The stamps panel shows each stamp's label (identifier in the tooltip), offers a download button per library that saves it as an Acrobat-readable PDF, and keeps custom libraries across reloads in IndexedDB — the built-in library is seeded on first open and never stored. A new `annotation:stamp-from-selection` command turns the selected annotation(s) into a stamp in a "My stamps" library, labelled `Custom stamp N` by default. English and Spanish strings added for the new controls.
