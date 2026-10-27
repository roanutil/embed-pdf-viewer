# Manage documents and workspace lifetime

Keep one workspace alive while your application uses its documents and capabilities.

## Own the workspace explicitly

Create a ``PDFViewer``, await `start()`, and then open documents. Retain the viewer in your app's document or workspace owner. Capabilities also retain the viewer, so dropping only the original variable may leave the engine alive.

Close a document when its tab closes. Cancel UI observation tasks and release models and capabilities that belong to it. An already acquired capability reports `instance-closed` after closure; asking the viewer for a capability for an absent document reports `not-found` instead. ``PDFViewer/pages(of:)`` also reports `not-found` for an absent document.

Await `destroy()` when the workspace ends. It closes documents, disposes plugins, stops the kernel thread, and finishes viewer streams. A successful destruction is terminal: subsequent awaited calls, including another Swift `destroy()` call, report `instance-closed`. Although the Rust kernel's disposal operation is idempotent, the Swift method also shuts down the host and cannot be used to restart it.

## Read document values as snapshots

``DocumentInfo`` is a value snapshot, not an observable live document. It contains `id`, optional `name`, `pageCount`, `status`, and optional failure `error`. Fetch `documents()` again after an unlock, page mutation, or other state change when you need updated values.

| Status | Meaning | Application action |
| --- | --- | --- |
| `.loading` | Opening is still in progress | Show progress and wait for lifecycle changes. |
| `.locked` | The document needs a password | Prompt and call `unlock(_:password:)`. |
| `.ready` | Pages and configured document plugins are available | Read pages and acquire capabilities. |
| `.failed` | Opening failed | Inspect `error`, then close the failed tab when dismissed. |

The same snapshot ID identifies the document after unlocking. Editing the snapshot's fields does not mutate the underlying document.

## Unlock a document without replacing its tab

Opening an encrypted document without a password can return `.locked`. An incorrect password supplied directly to `open` can instead throw and leave a failed tab. Prefer unlocking the existing locked document when presenting repeated password attempts.

The following helper accepts a locked snapshot and a password collected by your UI, then returns a fresh snapshot:

```swift
import EmbedPDF

enum UnlockExampleError: Error {
  case documentDisappeared
}

func unlockDocument(
  _ document: DocumentInfo,
  password: String,
  in viewer: PDFViewer
) async throws -> DocumentInfo {
  try await viewer.unlock(document, password: password)
  guard let updated = try await viewer.documents().first(where: {
    $0.id == document.id
  }) else {
    throw UnlockExampleError.documentDisappeared
  }
  return updated
}
```

A wrong unlock password throws a ``PDFError`` with `code == "permission-denied"` and leaves the tab locked so the app can retry. That code also covers other access refusals; only offer password recovery when your workflow knows it is unlocking a locked document.

## Refresh pages after editing

Use ``PageRef`` as identity and ``PageInfo``'s `index` as current order. Reordering changes indices, deletion removes references, and insertion introduces new pages. Subscribe to page changes or fetch a fresh layout after your mutation. A reference belongs to one open document; an object number in another file does not identify the same page.

## Verify lifetime handling

Open a document, obtain a capability, close the document, and ensure your UI removes its content and ends its tasks. In development, confirm that using the retained capability reports `instance-closed`, while the application keeps other open documents usable.

Next, use <doc:ErrorsAndObservation> to refresh snapshots from lifecycle events.
