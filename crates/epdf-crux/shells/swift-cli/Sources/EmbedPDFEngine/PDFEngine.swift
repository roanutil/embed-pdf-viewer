// The public surface, in the shape `multiplatform-plan-claude.md` sketches
// under "Native APIs that feel native". No Crux type appears in it.
import CoreGraphics
import EmbedPdfCrux
import Foundation

/// The engine's refusal, carrying the same `EngineErrorCode` spelling the web
/// engine reports.
public struct PDFError: Error, Sendable, Equatable, CustomStringConvertible {
    public struct Code: RawRepresentable, Hashable, Sendable {
        public let rawValue: String
        public init(rawValue: String) { self.rawValue = rawValue }

        public static let docPasswordRequired = Code(rawValue: "DocPasswordRequired")
        public static let docOpenFailed = Code(rawValue: "DocOpenFailed")
        public static let wireFormat = Code(rawValue: "WireFormat")
    }

    public let code: Code
    /// Diagnostic, and explicitly not a contract.
    public let message: String

    init(code: String, message: String) {
        self.code = Code(rawValue: code)
        self.message = message
    }

    public var description: String { "\(code.rawValue): \(message)" }
}

/// One engine: one PDFium owner thread and every document opened on it.
///
/// Released when the last reference goes, which closes its documents and
/// stops its thread. Documents keep their engine alive.
public final class PDFEngine: Sendable {
    let loop: CoreLoop

    public init() {
        loop = CoreLoop()
    }

    /// Opens a document from its bytes.
    ///
    /// Throws `PDFError` with `.docPasswordRequired` when the document needs
    /// a password it was not given, or was given the wrong one. The engine
    /// parks such a session, and the wire cannot yet unlock a parked one, so
    /// this closes it before throwing: call again with the password.
    public func open(data: Data, password: String? = nil) async throws -> PDFDocument {
        let docId = UUID().uuidString
        let view = await loop.open(docId: docId, document: loop.put(data), password: password)
        switch view.state {
        case .ready:
            return PDFDocument(engine: self, id: docId, pages: view.pages)
        case .needsPassword:
            try? loop.send(.close(docId: docId))
            throw PDFError(code: PDFError.Code.docPasswordRequired.rawValue, message: "a password is required")
        case .failed(let code, let message):
            try? loop.send(.close(docId: docId))
            throw PDFError(code: code, message: message)
        case .opening:
            preconditionFailure("open resumed before the document settled")
        }
    }
}

/// An open document. Its pages are known at open and addressed by durable
/// object number underneath, whatever index a caller asks for.
public final class PDFDocument: Sendable {
    let engine: PDFEngine
    let id: String
    private let views: [PageView]

    init(engine: PDFEngine, id: String, pages: [PageView]) {
        self.engine = engine
        self.id = id
        self.views = pages
    }

    public var pageCount: Int { views.count }

    /// Built on demand because each page points back at this document.
    public var pages: [PDFPage] { views.indices.map(page(at:)) }

    public func page(at index: Int) -> PDFPage {
        let view = views[index]
        return PDFPage(
            document: self,
            index: index,
            objectNumber: view.pageObjectNumber,
            size: CGSize(width: view.width, height: view.height))
    }

    /// Rejects new work on this document and asks the engine to close it.
    /// Returns before the engine has released it.
    public func close() {
        try? engine.loop.send(.close(docId: id))
    }

    deinit {
        close()
    }
}

/// One page. Cheap to make: it holds no engine state of its own.
public struct PDFPage: Sendable {
    let document: PDFDocument
    public let index: Int
    /// The PDF indirect object number, which is the durable key.
    public let objectNumber: UInt32
    /// In PDF points, with `/Rotate` normalized away.
    public let size: CGSize

    /// Renders at `scale` pixels per point.
    ///
    /// Cancelling the task ends the wait, not the render: the engine call is
    /// not preemptible, and its pixels are discarded when they arrive.
    public func render(scale: Double) async throws -> CGImage {
        let loop = document.engine.loop
        let effect = try await loop.request { request in
            .render(request: request, docId: document.id, pageObjectNumber: objectNumber, scale: scale)
        }
        switch effect {
        case .bitmap(let bitmap):
            guard let rgba = loop.take(bitmap.rgba), let image = cgImage(bitmap, rgba) else {
                throw PDFError(code: "WireFormat", message: "the render's pixels were missing")
            }
            return image
        case .failed(let failed):
            throw PDFError(code: failed.code, message: failed.message)
        default:
            preconditionFailure("a render was answered by \(effect)")
        }
    }

    /// The page's text in display order. UTF-16 offsets into it are the
    /// engine's offsets, so `NSString` ranges line up with the engine's.
    ///
    /// Cancellation ends the wait, not the read, as for `render`.
    public func text() async throws -> String {
        let effect = try await document.engine.loop.request { request in
            .text(request: request, docId: document.id, pageObjectNumber: objectNumber)
        }
        switch effect {
        case .text(let text):
            return text.text
        case .failed(let failed):
            throw PDFError(code: failed.code, message: failed.message)
        default:
            preconditionFailure("a text read was answered by \(effect)")
        }
    }
}

/// Wraps the engine's RGBA8, straight alpha, explicit stride.
private func cgImage(_ bitmap: PageBitmap, _ rgba: Data) -> CGImage? {
    guard let provider = CGDataProvider(data: rgba as CFData) else { return nil }
    return CGImage(
        width: Int(bitmap.width), height: Int(bitmap.height),
        bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(bitmap.stride),
        space: CGColorSpace(name: CGColorSpace.sRGB)!,
        bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
        provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
}
