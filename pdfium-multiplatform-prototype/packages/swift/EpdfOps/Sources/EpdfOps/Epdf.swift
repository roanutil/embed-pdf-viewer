import CoreGraphics
import Foundation

/// The whole Swift-side addition to what uniffi generates: one bridge from a
/// BGRA buffer to a CGImage, so a SwiftUI view can show a page without knowing
/// anything about pixel layout.
extension EpdfDocument {
    public func renderPageCGImage(index: UInt32, scale: Float) throws -> CGImage {
        let bitmap = try renderPage(index: index, scale: scale)
        // bitmap.bgra is already Data: uniffi maps Vec<u8> that way. And the
        // case is UpperCamelCase: uniffi 0.32.0 does NOT lowercase Rust enum
        // variant names in Swift. Verified against the generated
        // Sources/EpdfOps/Generated/epdf_uniffi.swift.
        return try bgraToCGImage(bgra: bitmap.bgra, width: bitmap.width, height: bitmap.height, stride: bitmap.stride)
    }
}

/// Wraps a BGRA buffer in a `CGImage`, byte order B,G,R,A per pixel. Split
/// out of `renderPageCGImage` so this channel logic can be exercised with a
/// synthetic buffer, not only through a real PDFium render; see
/// `bgraToCGImagePreservesChannelOrder` in VectorsTests.swift.
func bgraToCGImage(bgra: Data, width: UInt32, height: UInt32, stride: UInt32) throws -> CGImage {
    guard let provider = CGDataProvider(data: bgra as CFData) else {
        throw OpsError.RenderFailed
    }

    // PDFium's FPDFBitmap_BGRA is byte order B,G,R,A in memory, which on a
    // little-endian machine is a 32-bit little-endian ARGB word. Hence
    // .byteOrder32Little with premultipliedFirst.
    guard
        let image = CGImage(
            width: Int(width),
            height: Int(height),
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: Int(stride),
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedFirst.rawValue)
                .union(.byteOrder32Little),
            provider: provider,
            decode: nil,
            shouldInterpolate: false,
            intent: .defaultIntent
        )
    else {
        throw OpsError.RenderFailed
    }
    return image
}
