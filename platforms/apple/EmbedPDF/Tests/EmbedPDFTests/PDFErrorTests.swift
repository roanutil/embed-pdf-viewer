import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct PDFErrorTests {
    static let known: [(PDFError.Code, String)] = [
      (.permissionDenied, "permission-denied"),
      (.unsupported, "unsupported"),
      (.notFound, "not-found"),
      (.notReady, "not-ready"),
      (.invalidInput, "invalid-input"),
      (.conflict, "conflict"),
      (.instanceClosed, "instance-closed"),
      (.operationCancelled, "operation-cancelled"),
      (.operationFailed, "operation-failed"),
      (.invalidState, "invalid-state"),
    ]

    @Test(arguments: known) func eachKnownCodeRoundTripsItsWireString(code: PDFError.Code, wire: String) {
      expectNoDifference(code.rawValue, wire)
      expectNoDifference(PDFError.Code(rawValue: wire), code)
    }

    @Test func anUnknownCodeRoundTripsAsItsOwnString() {
      let code = PDFError.Code(rawValue: "quota-exceeded")
      expectNoDifference(code, .unknown("quota-exceeded"))
      expectNoDifference(code.rawValue, "quota-exceeded")
    }

    @Test func aKnownStringInUnknownIsTheKnownCode() {
      expectNoDifference(PDFError.Code.unknown("not-found"), .notFound)
    }

    @Test func theCodeTypedInitializerKeepsTheCode() {
      let error = PDFError(name: "PluginError", code: .permissionDenied, detail: "no")
      expectNoDifference(error.code, .permissionDenied)
    }

    @available(*, deprecated)
    @Test func theStringInitializerMapsKnownUnknownAndNilCodes() {
      expectNoDifference(PDFError(name: "n", code: "not-found" as String?, detail: "d").code, .notFound)
      expectNoDifference(PDFError(name: "n", code: "quota-exceeded" as String?, detail: "d").code, .unknown("quota-exceeded"))
      expectNoDifference(PDFError(name: "n", code: nil as String?, detail: "d").code, .operationFailed)
    }
  }
}
