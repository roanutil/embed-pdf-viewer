import CPDFium
import Dispatch
import Foundation

public enum PdfiumSwiftError: Error, Equatable {
    case loadFailed
    case password
    case pageOutOfRange(UInt32)
    case pageFailed
    case textFailed
    case renderFailed
    case closed
}

public struct PdfSize: Sendable, Equatable {
    public let width: Float
    public let height: Float
}

public struct PdfRect: Sendable, Equatable {
    public let left: Float
    public let top: Float
    public let right: Float
    public let bottom: Float
}

public struct PdfSearchHit: Sendable, Equatable {
    public let charIndex: UInt32
    public let charCount: UInt32
    public let rects: [PdfRect]
}

public struct PdfBitmap: Sendable, Equatable {
    public let width: UInt32
    public let height: UInt32
    public let stride: UInt32
    public let bgra: Data
}

private final class DocumentState: @unchecked Sendable {
    let handle: FPDF_DOCUMENT
    let bytes: UnsafeMutableRawPointer
    var closed = false

    init(handle: FPDF_DOCUMENT, bytes: UnsafeMutableRawPointer) {
        self.handle = handle
        self.bytes = bytes
    }
}

public final class PdfiumSwiftEngine: @unchecked Sendable {
    fileprivate let queue = DispatchQueue(label: "com.embedpdf.pdfium-swift")

    public init() {
        queue.sync { FPDF_InitLibrary() }
    }

    deinit {
        queue.sync { FPDF_DestroyLibrary() }
    }

    public func open(_ data: Data, password: String? = nil) throws -> PdfiumSwiftDocument {
        try queue.sync {
            let bytes = UnsafeMutableRawPointer.allocate(byteCount: max(data.count, 1), alignment: 16)
            data.copyBytes(to: bytes.assumingMemoryBound(to: UInt8.self), count: data.count)
            let handle = password?.withCString { FPDF_LoadMemDocument64(bytes, data.count, $0) }
                ?? FPDF_LoadMemDocument64(bytes, data.count, nil)
            guard let handle else {
                bytes.deallocate()
                if FPDF_GetLastError() == FPDF_ERR_PASSWORD { throw PdfiumSwiftError.password }
                throw PdfiumSwiftError.loadFailed
            }
            return PdfiumSwiftDocument(engine: self, state: DocumentState(handle: handle, bytes: bytes))
        }
    }
}

public final class PdfiumSwiftDocument: @unchecked Sendable {
    private let engine: PdfiumSwiftEngine
    private let state: DocumentState

    fileprivate init(engine: PdfiumSwiftEngine, state: DocumentState) {
        self.engine = engine
        self.state = state
    }

    private func withState<T>(_ body: (FPDF_DOCUMENT) throws -> T) throws -> T {
        try engine.queue.sync {
            guard !state.closed else { throw PdfiumSwiftError.closed }
            return try body(state.handle)
        }
    }

    public func pageCount() throws -> UInt32 {
        try withState { UInt32(max(FPDF_GetPageCount($0), 0)) }
    }

    public func pageSize(_ index: UInt32) throws -> PdfSize {
        try withState { document in
            try validate(index, in: document)
            var size = FS_SIZEF()
            guard EPDF_GetPageSizeByIndexNormalized(document, Int32(index), &size) != 0 else {
                throw PdfiumSwiftError.pageFailed
            }
            return PdfSize(width: size.width, height: size.height)
        }
    }

    public func pageText(_ index: UInt32) throws -> String {
        try withTextPage(index) { textPage in
            let count = max(FPDFText_CountChars(textPage), 0)
            var utf16 = [UInt16](repeating: 0, count: Int(count) * 2 + 1)
            let written = EPDFText_GetTextFull(textPage, &utf16, Int32(utf16.count))
            guard written > 0 else { return "" }
            return String(decoding: utf16.prefix(Int(written - 1)), as: UTF16.self)
        }
    }

    public func search(_ index: UInt32, query: String, caseSensitive: Bool) throws -> [PdfSearchHit] {
        var term = Array(query.utf16) + [0]
        return try withTextPage(index) { textPage in
            let flags: UInt = caseSensitive ? UInt(FPDF_MATCHCASE) : 0
            guard let search = FPDFText_FindStart(textPage, &term, flags, 0) else {
                throw PdfiumSwiftError.textFailed
            }
            defer { FPDFText_FindClose(search) }
            var hits: [PdfSearchHit] = []
            while FPDFText_FindNext(search) != 0 {
                let start = FPDFText_GetSchResultIndex(search)
                let count = FPDFText_GetSchCount(search)
                let rectangleCount = FPDFText_CountRects(textPage, start, count)
                var rectangles: [PdfRect] = []
                rectangles.reserveCapacity(Int(max(rectangleCount, 0)))
                for rectangle in 0..<max(rectangleCount, 0) {
                    var left = 0.0, top = 0.0, right = 0.0, bottom = 0.0
                    _ = FPDFText_GetRect(textPage, rectangle, &left, &top, &right, &bottom)
                    rectangles.append(PdfRect(left: Float(left), top: Float(top), right: Float(right), bottom: Float(bottom)))
                }
                hits.append(PdfSearchHit(charIndex: UInt32(max(start, 0)), charCount: UInt32(max(count, 0)), rects: rectangles))
            }
            return hits
        }
    }

    public func renderPage(_ index: UInt32, scale: Float) throws -> PdfBitmap {
        try withState { document in
            try validate(index, in: document)
            guard scale.isFinite, scale > 0, let page = FPDF_LoadPage(document, Int32(index)) else {
                throw PdfiumSwiftError.renderFailed
            }
            defer { FPDF_ClosePage(page) }
            let width = max(Int32((FPDF_GetPageWidthF(page) * scale).rounded()), 1)
            let height = max(Int32((FPDF_GetPageHeightF(page) * scale).rounded()), 1)
            let stride = width.multipliedReportingOverflow(by: 4)
            let byteCount = Int64(stride.partialValue).multipliedReportingOverflow(by: Int64(height))
            guard !stride.overflow, !byteCount.overflow, byteCount.partialValue <= 256 * 1024 * 1024 else {
                throw PdfiumSwiftError.renderFailed
            }
            var pixels = Data(count: Int(byteCount.partialValue))
            try pixels.withUnsafeMutableBytes { raw in
                guard let bitmap = FPDFBitmap_CreateEx(width, height, FPDFBitmap_BGRA, raw.baseAddress, stride.partialValue) else {
                    throw PdfiumSwiftError.renderFailed
                }
                defer { FPDFBitmap_Destroy(bitmap) }
                _ = FPDFBitmap_FillRect(bitmap, 0, 0, width, height, 0xffff_ffff)
                FPDF_RenderPageBitmap(bitmap, page, 0, 0, width, height, 0, 0)
            }
            return PdfBitmap(width: UInt32(width), height: UInt32(height), stride: UInt32(stride.partialValue), bgra: pixels)
        }
    }

    public func close() {
        engine.queue.sync {
            guard !state.closed else { return }
            state.closed = true
            FPDF_CloseDocument(state.handle)
            state.bytes.deallocate()
        }
    }

    deinit { close() }

    private func validate(_ index: UInt32, in document: FPDF_DOCUMENT) throws {
        guard index < UInt32(max(FPDF_GetPageCount(document), 0)) else {
            throw PdfiumSwiftError.pageOutOfRange(index)
        }
    }

    private func withTextPage<T>(_ index: UInt32, _ body: (FPDF_TEXTPAGE) throws -> T) throws -> T {
        try withState { document in
            try validate(index, in: document)
            guard let page = FPDF_LoadPage(document, Int32(index)) else { throw PdfiumSwiftError.pageFailed }
            defer { FPDF_ClosePage(page) }
            guard let textPage = FPDFText_LoadPage(page) else { throw PdfiumSwiftError.textFailed }
            defer { FPDFText_ClosePage(textPage) }
            return try body(textPage)
        }
    }
}
