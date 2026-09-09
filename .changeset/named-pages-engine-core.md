---
'@embedpdf/engine-core': minor
---

Named pages join the page list. `PageListSnapshot.namedPages` carries the catalog's `/Names /Pages` and `/Names /Templates` registrations in tree order — `NamedPageEntry` is the decoded key plus a target classified as `page` (a page in `pages`), `template` (a hidden `/Type /Template` page that is never listed or rendered), or `dangling`. Because a registration only means something against the page set that contains its target, it is layout data like `label` and shares the layout version rather than a plane of its own.

`DocumentPagesService` gains two optional page-structure mutations: `setName({ name, pageObjectNumber, replace? })` registers a key, replaces what an existing key points at, or renames in one job; `removeName({ name })` drops a registration and keeps the page. Both return the fresh layout as `PageNameResult` (layout plus the docVersion/layoutVersion pins) and publish a `pages.named` event. Deleting a page removes every registration pointing at it. `runNamedPagesConformance` locks these invariants for every engine. The engine never interprets key text.

Stamp `/Name` accepts any non-empty name — a standard stamp name or a custom identifier such as an Acrobat library's `#…` key — and a stamp patch may clear it with `name: null`.
