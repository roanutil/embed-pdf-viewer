// The Core Graphics surface the EmbedPDF sources and tests touch, enough to
// type-check and run on Linux. Images keep their bytes; a bitmap context
// copies an image row by row (no colour management, no premultiplication).
@_exported import AppleCFStub
@_exported import Foundation
// URLSession lives in FoundationNetworking on Linux; Apple's Foundation includes it.
@_exported import FoundationNetworking

public final class CGColorSpace: @unchecked Sendable {
  public static let sRGB: CFString = "kCGColorSpaceSRGB"
  public let name: CFString?
  public init?(name: CFString) { self.name = name }
}

public final class CGColor: @unchecked Sendable {
  public let colorSpace: CGColorSpace?
  public let components: [CGFloat]?
  public init(srgbRed red: CGFloat, green: CGFloat, blue: CGFloat, alpha: CGFloat) {
    colorSpace = CGColorSpace(name: CGColorSpace.sRGB)
    components = [red, green, blue, alpha]
  }
}

public enum CGImageAlphaInfo: UInt32, Sendable {
  case none = 0, premultipliedLast, premultipliedFirst, last, first, noneSkipLast, noneSkipFirst, alphaOnly
}

public struct CGBitmapInfo: OptionSet, Sendable {
  public let rawValue: UInt32
  public init(rawValue: UInt32) { self.rawValue = rawValue }
}

public enum CGColorRenderingIntent: Int32, Sendable { case defaultIntent = 0 }

public final class CGDataProvider: @unchecked Sendable {
  public let data: Data
  public init?(data: CFData) { self.data = data }
}

public final class CGImage: @unchecked Sendable {
  public let width: Int
  public let height: Int
  public let bytesPerRow: Int
  public let bitsPerPixel: Int
  public let bitmapInfo: CGBitmapInfo
  public let dataProvider: CGDataProvider?
  public init?(
    width: Int, height: Int, bitsPerComponent: Int, bitsPerPixel: Int, bytesPerRow: Int,
    space: CGColorSpace, bitmapInfo: CGBitmapInfo, provider: CGDataProvider,
    decode: UnsafePointer<CGFloat>?, shouldInterpolate: Bool, intent: CGColorRenderingIntent
  ) {
    guard width > 0, height > 0, bytesPerRow >= width * bitsPerPixel / 8,
      provider.data.count >= bytesPerRow * (height - 1) + width * bitsPerPixel / 8
    else { return nil }
    self.width = width
    self.height = height
    self.bytesPerRow = bytesPerRow
    self.bitsPerPixel = bitsPerPixel
    self.bitmapInfo = bitmapInfo
    dataProvider = provider
  }
}

public final class CGContext {
  let data: UnsafeMutableRawPointer
  let width: Int
  let height: Int
  let bytesPerRow: Int
  public init?(
    data: UnsafeMutableRawPointer?, width: Int, height: Int, bitsPerComponent: Int,
    bytesPerRow: Int, space: CGColorSpace, bitmapInfo: UInt32
  ) {
    guard let data, bitsPerComponent == 8 else { return nil }
    self.data = data
    self.width = width
    self.height = height
    self.bytesPerRow = bytesPerRow
  }

  /// Copies `image` at its own size from the top-left; `rect` must match it.
  public func draw(_ image: CGImage, in rect: CGRect) {
    guard let pixels = image.dataProvider?.data, image.bitsPerPixel == 32 else { return }
    let rows = min(height, image.height)
    let bytes = min(width, image.width) * 4
    pixels.withUnsafeBytes { source in
      for row in 0..<rows {
        (data + row * bytesPerRow).copyMemory(
          from: source.baseAddress! + row * image.bytesPerRow, byteCount: bytes)
      }
    }
  }
}
