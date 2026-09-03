import CoreGraphics
import Foundation
import Observation
import EpdfOps

/// All six operations, driven off the main thread. `load()` hands the open
/// and all five reads to one detached task; only the finished values cross
/// back to the main actor, in a single call.
@Observable
@MainActor
final class DocumentModel {
    var pageCount: UInt32 = 0
    var pageSize: Size?
    var image: CGImage?
    var textPreview: String = ""
    var searchHitCount: Int = 0
    var failure: String?

    private let pageIndex: UInt32 = 4
    private var document: EpdfDocument?
    private var loadTask: Task<Void, Never>?
    private var closed = false
    private var generation = 0

    private struct MissingResource: Error {}

    func load() {
        // Cancel and disown whatever load is already in flight. If it's past
        // the point where cancellation is checked and still calls back, its
        // generation won't match the one started below, so `finishLoad` and
        // `finishFailure` close its document instead of storing it.
        loadTask?.cancel()
        closed = false
        generation += 1
        let currentGeneration = generation
        let pageIndex = self.pageIndex

        loadTask = Task.detached { [weak self] in
            var openedDocument: EpdfDocument?
            do {
                guard let url = Bundle.main.url(forResource: "report", withExtension: "pdf") else {
                    throw MissingResource()
                }
                let bytes = try Data(contentsOf: url)
                let doc = try EpdfEngine().open(bytes: bytes, password: nil)
                openedDocument = doc

                let count = try doc.pageCount()
                let size = try doc.pageSize(index: pageIndex)
                try Task.checkCancellation()
                let text = String(try doc.pageText(index: pageIndex).prefix(600))
                let hits = try doc.search(index: pageIndex, query: "the", caseSensitive: false).count
                try Task.checkCancellation()
                let image = try doc.renderPageCGImage(index: pageIndex, scale: 2.0)

                await self?.finishLoad(
                    document: doc,
                    generation: currentGeneration,
                    pageCount: count,
                    pageSize: size,
                    textPreview: text,
                    searchHitCount: hits,
                    image: image
                )
            } catch is CancellationError {
                // The view navigated away; this isn't a failure worth
                // reporting, just work to discard.
                openedDocument?.close()
            } catch is MissingResource {
                await self?.finishFailure(generation: currentGeneration, "report.pdf is not in the app bundle")
            } catch {
                openedDocument?.close()
                await self?.finishFailure(generation: currentGeneration, String(describing: error))
            }
        }
    }

    /// Called back on the main actor once the detached task has a document
    /// and all five reads. Discards the document instead of storing it if
    /// `close()` ran while that work was in flight, or if a later `load()`
    /// has since started a new generation, so no document ever ends up
    /// leaked or stored over a newer one.
    private func finishLoad(
        document doc: EpdfDocument,
        generation: Int,
        pageCount: UInt32,
        pageSize: Size,
        textPreview: String,
        searchHitCount: Int,
        image: CGImage
    ) {
        guard !closed, generation == self.generation else {
            doc.close()
            return
        }
        document = doc
        self.pageCount = pageCount
        self.pageSize = pageSize
        self.textPreview = textPreview
        self.searchHitCount = searchHitCount
        self.image = image
    }

    private func finishFailure(generation: Int, _ message: String) {
        guard !closed, generation == self.generation else { return }
        failure = message
    }

    func close() {
        closed = true
        loadTask?.cancel()
        document?.close()
        document = nil
    }
}
