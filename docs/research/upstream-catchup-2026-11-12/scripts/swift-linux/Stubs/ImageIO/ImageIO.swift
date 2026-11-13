// ImageIO on Linux: no decoder, so every source reads as unreadable.
@_exported import CoreGraphics

public final class CGImageSource {}

public func CGImageSourceCreateWithData(_ data: CFData, _ options: CFDictionary?) -> CGImageSource? { nil }

public func CGImageSourceCreateImageAtIndex(
  _ source: CGImageSource, _ index: Int, _ options: CFDictionary?
) -> CGImage? { nil }
