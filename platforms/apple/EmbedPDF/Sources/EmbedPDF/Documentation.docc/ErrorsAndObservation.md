# Handle errors and observe changes

Recover from stable error codes and use events to refresh application state.

## Handle awaited failures at the call site

Viewer and capability methods use typed `throws(PDFError)`. Match ``PDFError/code`` for recovery; `name` and `detail` are diagnostics. A diagnostic can contain an engine name such as `DocPasswordIncorrect`, but parsing that text is not a general error-handling strategy.

| Code | Meaning | Typical response |
| --- | --- | --- |
| `permission-denied` | Scope, PDF permissions, password, or signature policy refused access | Explain the restriction; retry only with an appropriate changed condition. |
| `unsupported` | A required plugin or engine feature is unavailable | Check configuration and supported workflows. |
| `not-found` | The requested resource does not exist | Refresh the resource list and remove stale UI. |
| `not-ready` | A document is loading, locked, or failed | Inspect its status before acquiring capabilities. |
| `invalid-input` | Invalid arguments or PDF input | Correct the input and retry. |
| `conflict` | State changed during the operation | Refresh affected state before deciding whether to retry. |
| `instance-closed` | A retained document capability or viewer has closed | End the operation and release its UI. |
| `operation-cancelled` | A cancellation-aware operation was aborted | End progress without presenting an ordinary failure alert. |
| `operation-failed` | Another operational failure occurred | Record diagnostics and offer recovery appropriate to the task. |
| `invalid-state` | A value could not cross the native boundary correctly | Record a reproducible SDK issue; do not treat it as invalid user input. |

Viewer-thrown errors contain a code. The public initializer also allows application-defined callback failures without one. File I/O or application helpers around SDK calls can throw other error types.

## Respect each operation's cancellation contract

`async` alone does not promise cancellation or rollback. The Swift wrappers pass an explicit cancellation token only for operations wired to support it.

| Operation | Cancellation behavior |
| --- | --- |
| Save, whole-page render, thumbnail render, and `findAll` | Cancellation-aware calls can throw `operation-cancelled`. |
| Session `search` and `refresh` | A cancelled scan can return a cancelled result and retain hits found so far. |
| Command execution | Cancelling the Swift task does not cancel the command. |
| Event stream iteration | Cancelling its consuming task terminates that subscription; it does not cancel unrelated operations. |
| A mutation already sent to the engine | Cancellation is not a general rollback guarantee; refresh state before retrying. |

Check the relevant method's documentation for other operations. Cancel and await owned UI work before closing the document it uses.

## Observe document events

Read ``PDFViewer/documentEvents`` before the operation whose changes you need to observe. Each access creates a subscription with no replay of past events. A full state list and a stream serve different purposes: use `documents()` for a current snapshot and events to trigger later refreshes.

This function is an observation loop for an already started viewer. Its caller owns the task and cancels it when the corresponding UI goes away:

```swift
import EmbedPDF

func observeDocuments(in viewer: PDFViewer) async {
  let events = viewer.documentEvents
  for await event in events {
    switch event {
    case let .opened(document):
      print("Opened", document.id)
    case let .closed(documentID):
      print("Closed", documentID)
    case let .pagesChanged(documentID):
      print("Refresh page layout for", documentID)
    default:
      break
    }
  }
}
```

If the caller launches a new task containing this function, scheduling that task does not itself guarantee subscription before the caller's next operation. For strict ordering, obtain the stream synchronously first and then hand it to your consumer before opening the document.

Document-scoped capability streams finish when their document closes or the viewer is destroyed. Cancelling iteration unregisters the listener. Consume each individual stream with one iterator; obtain separate streams for independent subscribers.

## Observe errors and state revisions separately

``PDFViewer/reportedErrors`` handles failures without a throwing return path, including listener errors and queued stage camera operations. It keeps the newest 16 errors and has one consumer. It does not replace `catch` around awaited operations.

``PDFViewer/stateRevisions`` buffers only the newest state revision. It is a refresh signal, not an audit log and not a promise that you will receive every intermediate revision. Keep one consumer and read the relevant snapshots after a notification. Both streams finish when viewer destruction succeeds.

## Verify error presentation

Try a restricted scope, a locked document, and a cancelled search. Ensure your app distinguishes permission refusal, password entry, and cancellation. Then remove the screen and verify that the observation task finishes or is cancelled rather than continuing to update released UI.

Next, follow <doc:ExtendingTheViewer> to supply callbacks that report failures through the same model.
