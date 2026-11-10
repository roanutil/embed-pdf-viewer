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

    @Test func anUnnamedCodeRoundTripsAsItsOwnString() {
      let code = PDFError.Code(rawValue: "quota-exceeded")
      expectNoDifference(code, "quota-exceeded")
      expectNoDifference(code.rawValue, "quota-exceeded")
      #expect(!Self.known.map(\.0).contains(code))
    }

    @Test func aStringLiteralComparesWithACode() {
      let error = PDFError(.notFound, detail: "no page")
      #expect(error.code == "not-found")
      #expect(error.code != "not-ready")
    }

    @Test func theCodeTypedInitializerKeepsTheCode() {
      let error = PDFError(name: "PluginError", code: .permissionDenied, detail: "no")
      expectNoDifference(error.code, .permissionDenied)
    }

    @Test func theShortInitializerNamesItAPluginError() {
      expectNoDifference(
        PDFError(.permissionDenied, detail: "no"),
        PDFError(name: "PluginError", code: .permissionDenied, detail: "no"))
      expectNoDifference(PDFError(.conflict, detail: "d", name: "Custom").name, "Custom")
    }

    @Test func aCatchMatchesTheCode() {
      func outcome(_ code: PDFError.Code) -> String {
        do {
          throw PDFError(code, detail: "d")
        } catch PDFError.Code.notFound {
          return "not found"
        } catch PDFError.Code.permissionDenied {
          return "denied"
        } catch {
          return "other"
        }
      }
      expectNoDifference([outcome(.notFound), outcome(.permissionDenied), outcome(.conflict)], ["not found", "denied", "other"])
    }

    @Test func aTypedCatchMatchesTheCode() async {
      func outcome(_ code: PDFError.Code) async -> String {
        do throws(PDFError) {
          try await { () async throws(PDFError) in throw PDFError(code, detail: "d") }()
          return "none"
        } catch PDFError.Code.notFound {
          return "not found"
        } catch {
          return "other"
        }
      }
      let found = await outcome(.notFound)
      let other = await outcome(.conflict)
      expectNoDifference([found, other], ["not found", "other"])
    }

    @Test func aForeignErrorNeverMatchesACode() {
      struct Foreign: Error {}
      #expect(!(PDFError.Code.operationFailed ~= Foreign()))
    }

    @Test func localizedDescriptionIsTheDetail() {
      let error: any Error = PDFError(.notFound, detail: "no page p9")
      expectNoDifference(error.localizedDescription, "no page p9")
    }

    @Test func descriptionLeadsWithTheCode() {
      expectNoDifference("\(PDFError(.notFound, detail: "no page p9"))", "not-found: no page p9")
    }

    @available(*, deprecated)
    @Test func theStringInitializerMapsKnownUnknownAndNilCodes() {
      expectNoDifference(PDFError(name: "n", code: "not-found" as String?, detail: "d").code, .notFound)
      expectNoDifference(PDFError(name: "n", code: "quota-exceeded" as String?, detail: "d").code, "quota-exceeded")
      expectNoDifference(PDFError(name: "n", code: nil as String?, detail: "d").code, .operationFailed)
    }
  }
}
