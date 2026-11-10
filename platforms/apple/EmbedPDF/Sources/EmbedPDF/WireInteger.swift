// The public surface counts and indexes in `Int`; the engine takes and gives
// fixed-width integers (`UInt32` for a hit, stage page, pane or tab index,
// `Int64` for the viewer's page index and document-order position). These
// convert at that seam without trapping.

extension BinaryInteger {
  /// This value as the engine's fixed-width `Wire` type. A value outside
  /// `Wire`'s range (below zero for an unsigned one, or too big) is the
  /// caller's mistake, so it throws `invalid-input` naming `argument`.
  package func toWire<Wire: FixedWidthInteger>(_ argument: String, as _: Wire.Type = Wire.self) throws(PDFError)
    -> Wire
  {
    guard let wire = Wire(exactly: self) else {
      throw PDFError(
        name: "RangeError", code: .invalidInput,
        detail: "\(argument) \(self) is outside \(Wire.min)...\(Wire.max)")
    }
    return wire
  }

  /// A count or index the engine answered, as `Int`. It always fits on the
  /// 64-bit platforms the package supports; one that doesn't is a value
  /// that didn't survive the boundary, so it throws `invalid-state`.
  package func toInt(_ result: String) throws(PDFError) -> Int {
    guard let value = Int(exactly: self) else {
      throw PDFError(
        name: "RangeError", code: .invalidState,
        detail: "\(result) \(self) is outside \(Int.min)...\(Int.max)")
    }
    return value
  }
}
