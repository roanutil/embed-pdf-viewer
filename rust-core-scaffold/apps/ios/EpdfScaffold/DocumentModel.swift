import CoreGraphics
import Foundation
import Observation
import EpdfOps

/// All six operations, driven off the main thread. `load()` hands the open
/// and all five reads to one detached task; only the finished values cross
/// back to the main actor, in a single assignment.
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

    private struct MissingResource: Error {}

    func load() {
        let pageIndex = self.pageIndex
        loadTask = Task.detached { [weak self] in
            do {
                guard let url = Bundle.main.url(forResource: "report", withExtension: "pdf") else {
                    throw MissingResource()
                }
                let bytes = try Data(contentsOf: url)
                let doc = try EpdfEngine().open(bytes: bytes, password: nil)

                let count = try doc.pageCount()
                let size = try doc.pageSize(index: pageIndex)
                let text = String(try doc.pageText(index: pageIndex).prefix(600))
                let hits = try doc.search(index: pageIndex, query: "the", caseSensitive: false).count
                let image = try doc.renderPageCGImage(index: pageIndex, scale: 2.0)

                await self?.finishLoad(
                    document: doc,
                    pageCount: count,
                    pageSize: size,
                    textPreview: text,
                    searchHitCount: hits,
                    image: image
                )
            } catch is MissingResource {
                await self?.finishFailure("report.pdf is not in the app bundle")
            } catch {
                await self?.finishFailure(String(describing: error))
            }
        }
    }

    /// Called back on the main actor once the detached task has a document
    /// and all five reads. If `close()` already ran while that work was in
    /// flight, the freshly opened document is closed instead of stored, so
    /// no document ever ends up leaked.
    private func finishLoad(
        document doc: EpdfDocument,
        pageCount: UInt32,
        pageSize: Size,
        textPreview: String,
        searchHitCount: Int,
        image: CGImage
    ) {
        guard !closed else {
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

    private func finishFailure(_ message: String) {
        guard !closed else { return }
        failure = message
    }

    func close() {
        closed = true
        loadTask?.cancel()
        document?.close()
        document = nil
    }
}
