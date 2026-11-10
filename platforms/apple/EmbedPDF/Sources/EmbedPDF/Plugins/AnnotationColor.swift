public import CoreGraphics

/// An annotation colour: `/C` and the colours drafts and patches write.
///
/// The engine reads and writes colours as `"#rrggbb"` (`Rgb` in
/// `crates/epdf-engine/src/features/annotation/write/prepare.rs`): six hex
/// digits in either case on the way in, lowercase on the way out. This type
/// holds the three channels, so a misspelled colour is caught where it is
/// written instead of as `invalid-input` from the engine.
///
/// A string literal is a colour: `color: "#ff0000"`. A literal that isn't
/// `#rrggbb` stops the program, as a malformed literal is a typo in the
/// source; for text from elsewhere, use ``init(hex:)``, which returns `nil`.
public struct AnnotationColor: Hashable, Sendable, Codable, ExpressibleByStringLiteral,
  CustomStringConvertible
{
  public var red: UInt8
  public var green: UInt8
  public var blue: UInt8

  public init(red: UInt8, green: UInt8, blue: UInt8) {
    self.red = red
    self.green = green
    self.blue = blue
  }

  /// The colour `"#rrggbb"` names, in either case, or `nil` for any other
  /// text: the engine's own rule.
  public init?(hex: String) {
    let digits = Array(hex.utf8)
    guard digits.count == 7, digits[0] == UInt8(ascii: "#") else { return nil }
    func channel(_ offset: Int) -> UInt8? {
      guard let high = Self.nibble(digits[offset]), let low = Self.nibble(digits[offset + 1]) else { return nil }
      return high << 4 | low
    }
    guard let red = channel(1), let green = channel(3), let blue = channel(5) else { return nil }
    self.init(red: red, green: green, blue: blue)
  }

  public init(stringLiteral hex: String) {
    guard let color = Self(hex: hex) else {
      preconditionFailure("\"\(hex)\" isn't an annotation colour: write \"#rrggbb\"")
    }
    self = color
  }

  /// `"#rrggbb"`, lowercase, as the engine writes it.
  public var hex: String {
    let digits = Array("0123456789abcdef".utf8)
    var text: [UInt8] = [UInt8(ascii: "#")]
    for channel in [red, green, blue] {
      text.append(digits[Int(channel >> 4)])
      text.append(digits[Int(channel & 0x0f)])
    }
    return String(decoding: text, as: UTF8.self)
  }

  public var description: String { hex }

  /// The colour in the sRGB colour space, opaque. An annotation's opacity is
  /// its own member.
  public var cgColor: CGColor {
    CGColor(
      srgbRed: CGFloat(red) / 255, green: CGFloat(green) / 255, blue: CGFloat(blue) / 255, alpha: 1)
  }

  public init(from decoder: any Decoder) throws {
    let container = try decoder.singleValueContainer()
    let text = try container.decode(String.self)
    guard let color = Self(hex: text) else {
      throw DecodingError.dataCorruptedError(
        in: container, debugDescription: "\"\(text)\" isn't a colour as \"#rrggbb\"")
    }
    self = color
  }

  public func encode(to encoder: any Encoder) throws {
    var container = encoder.singleValueContainer()
    try container.encode(hex)
  }

  private static func nibble(_ byte: UInt8) -> UInt8? {
    switch byte {
    case UInt8(ascii: "0")...UInt8(ascii: "9"): byte - UInt8(ascii: "0")
    case UInt8(ascii: "a")...UInt8(ascii: "f"): byte - UInt8(ascii: "a") + 10
    case UInt8(ascii: "A")...UInt8(ascii: "F"): byte - UInt8(ascii: "A") + 10
    default: nil
    }
  }
}
