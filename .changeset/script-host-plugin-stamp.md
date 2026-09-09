---
'@embedpdf/plugin-stamp': patch
---

Dynamic stamp evaluation rides the workspace's ONE JavaScript switch. `StampConfig.scripting` / `StampScriptingOptions` are REMOVED: on arm, a form-backed PDF asset asks the target document's actions plugin for a DETACHED realm (`createDetachedScriptRealm` — same identity, clock, sandbox, and budget as the viewer document; its own isolated globals), recalculates an isolated copy, flattens it, and arms the result; script alerts and diagnostics surface through the actions port tagged `realm: 'detached'`. With scripting off, or without the actions plugin, the template is armed unevaluated. The new `dynamic: false` keeps templates static even with scripting on — a product choice, not a trust boundary.
