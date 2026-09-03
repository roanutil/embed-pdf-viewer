import CoreGraphics
import Foundation
import Observation
import EpdfOps

/// All six operations, driven off the main thread. epdf-ops routes every call
/// to the one thread that owns the PDFium handle, so this actor's job is only
/// to keep the UI responsive, not to protect the handle.
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

    func load() async {
        do {
            guard let url = Bundle.main.url(forResource: "report", withExtension: "pdf") else {
                failure = "report.pdf is not in the app bundle"
                return
            }
            let bytes = try Data(contentsOf: url)

            let doc = try await Task.detached { try EpdfEngine().open(bytes: bytes, password: nil) }.value
            document = doc

            pageCount = try doc.pageCount()
            pageSize = try doc.pageSize(index: pageIndex)
            textPreview = String(try doc.pageText(index: pageIndex).prefix(600))
            searchHitCount = try doc.search(index: pageIndex, query: "the", caseSensitive: false).count
            image = try doc.renderPageCGImage(index: pageIndex, scale: 2.0)
        } catch {
            failure = String(describing: error)
        }
    }

    func close() {
        document?.close()
        document = nil
    }
}
