# Add search controls

Bind a search field and hit navigation to one document's search session.

## Before you begin

Start a viewer configured with `SearchConfig()` and open a document as described
in **Render your first PDF page** in the `EmbedPDF` module documentation.
Search permission is required; a configuration
flag enables the plugin but does not grant session permissions. This example
uses the default query options and lets the capability choose its default hit
reveal behavior.

## Retain the model and connect controls

Create `SearchModel(search: try await viewer.search(in: document))` on the main
actor and retain it in your screen's SwiftUI state. Handle capability-acquisition
errors there, then pass the model to this complete integration excerpt in
`DocumentSearchControls.swift`.

```swift
import EmbedPDF
import EmbedPDFUI
import SwiftUI

@MainActor
struct DocumentSearchControls: View {
  @Bindable var model: SearchModel
  @State private var operation: Task<Void, Never>?
  @State private var isWorking = false

  var body: some View {
    VStack {
      TextField("Search document", text: $model.text)
        .submitLabel(.search)
        .onSubmit { submit() }
      HStack {
        Button("Search") { submit() }
        Button("Previous") {
          run { await model.previousButtonTapped() }
        }
        Button("Next") {
          run { await model.nextButtonTapped() }
        }
        Button("Clear results") {
          run { await model.clearButtonTapped() }
        }
      }
      .disabled(isWorking)
      Text("\(model.hits.count) matches")
      if let index = model.activeIndex {
        Text("Match \(Int(index) + 1)")
      }
      if isWorking { ProgressView("Searching or updating results") }
      if let error = model.error {
        Text(error.detail).foregroundStyle(.red)
      }
    }
    .task { await model.task() }
    .onDisappear { operation?.cancel() }
  }

  private func submit() {
    run { await model.searchSubmitted() }
  }

  private func run(_ action: @escaping @MainActor () async -> Void) {
    guard !isWorking else { return }
    isWorking = true
    operation = Task { @MainActor in
      defer { isWorking = false }
      await action()
    }
  }
}
```

This host serializes its own button actions and submissions. The activity
indicator represents the host operation, because ``SearchModel/status`` does
not publish intermediate scan progress. Cancelling an in-flight submission
cancels the underlying scan, which may return the hits collected so far and a
cancelled status rather than throwing. A thrown capability error appears in
``SearchModel/error`` and a failed submission sets the model status to `.error`.

The hit count is a snapshot after submission, not a live progress count.
`activeIndex` is zero-based within the session's hit list, so its label adds one.
The example displays counts and navigation; it does not draw highlight overlays
on ``StageReaderView`` or automatically create a results list.

## Keep one owner for session state

``SearchModel/task()`` reads initial status and active index, then follows
active-hit changes, including navigation made through another capability.
It does not fetch existing hits at startup or follow external query, hit-list,
or status changes. Route search and clear operations through this model if its
snapshots drive your UI. Use a new model for a newly opened document; changing
the field alone does not run a search.

“Clear results” drops the underlying session, empties the model's hits, and
removes its active index. It deliberately leaves the field text unchanged.
To clear the field too, set `model.text = ""` in the host. A successful clear or
navigation also leaves an earlier error visible; only a successful submission
clears the model's stored error. If you need a dismissible current-operation
error presentation, own that presentation separately in your screen.

## Stop UI work before closing the document

SwiftUI cancels the event-observation task on removal, and the example cancels
its owned control task. For a screen that also closes the document, retain and
await any owned operation task's completion before closing. Cancelling the
observation task alone does not clear the search session or cancel a separately
running search. If the document stays open and should lose its search results,
call ``SearchModel/clearButtonTapped()`` before releasing the model.

The document owner handles errors from `PDFViewer.close(_:)` and
`PDFViewer.destroy()`. Models retain capabilities and therefore the viewer,
but explicit closure still invalidates those capabilities. A model is not a
replacement for document lifetime management.

## Verify the result

Search for a word present in your PDF. The count should become nonzero and the
active label should show “Match 1.” Next and Previous should change that index;
Clear results should show zero matches while preserving the field text. For an
unexpected empty result, check that the PDF contains extractable text rather
than only scanned images, and inspect any reported permission or lifetime error.

## Related APIs

- ``SearchModel``
- ``SearchModel/searchSubmitted()``
- ``SearchModel/task()``
- <doc:DisplaySwiftUIReader>
