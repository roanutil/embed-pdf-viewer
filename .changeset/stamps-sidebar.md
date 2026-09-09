---
'@embedpdf/plugin-annotation': minor
'@embedpdf/plugin-stamp': minor
'@embedpdf/viewer-chrome': minor
'@embedpdf/viewer': minor
---

The Insert tab's stamp button now opens a stamps SIDEBAR — a picker over the stamp plugin's libraries — instead of activating a click-then-pick file dialog. Picking a stamp arms it; hovering a page ghosts the exact placement and each click places one. The viewer registers `stampPlugin()` and seeds a built-in set of the classic rubber stamps (drawn on first open, never at boot), and the panel imports any PDF as a library (one vector stamp per page). Arbitrary image bytes moved to the Insert tab's Image button, now a real `stamp` preset whose `source` prompts the file-picker port for PNG/JPEG — the gesture the stamp button used to have. `StampToken` joins the drive door's exported tokens.

`plugin-stamp` gains `createLibrary(name, opts?)`: the counterpart of `removeLibrary`, and the only way to name a loose library (`addAsset` without a `libraryId` names one after its first asset).

`plugin-annotation` fixes `armStamp` disarming the payload it had just set. The tool-change guard matched only the legacy `annotation-stamp` tag, which no built-in tool carries, so `armStamp`'s own `activateTool('stamp')` cleared the arm on every call made from another tool — placement then silently fell through to the tool's `source` spec. The guard now keys on the tool `armStamp` activates (`ARMED_STAMP_TOOL_ID`), still honouring the legacy tag; a payload no longer survives onto a sibling stamp preset whose own `source` it would hijack.
