import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct ActionsTests {
    func opened(_ fixture: String = "document_actions") async throws -> (PDFViewer, DocumentInfo, ActionsCapability) {
      let viewer = try PDFViewer(ViewerConfig(actions: ActionsConfig(openSequence: .off)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data(fixture))
      return (viewer, document, try await viewer.actions(for: document))
    }

    static let user = ActionContext(origin: .user, source: .api, event: .activate)

    static func uriTree(_ uri: String) throws -> PdfActionTree {
      try PdfActionTree.decode(
        json: #"{"root":{"type":"uri","subtype":"URI","uri":"\#(uri)","isMap":false,"next":[]},"incomplete":false,"warningFlags":0,"warnings":[]}"#
      )
    }

    @Test func theCatalogsOpenTreeIsJavaScript() async throws {
      let (_, _, actions) = try await opened()
      let open = try await actions.actionTree(for: .document(event: nil))
      expectNoDifference(open?.rootType(), "javascript")
      let scripting = try await actions.isScriptingEnabled()
      expectNoDifference(scripting, false)
    }

    @Test func aUserUriReachesTheHandlerAndAFileUriDoesNot() async throws {
      let (_, _, actions) = try await opened()
      let ui = RecordingUI()
      let registration = try await actions.setUIHandler(ui)
      _ = try await actions.execute(Self.uriTree("https://a.test/"), context: Self.user)
      let blocked = try await actions.execute(Self.uriTree("file:///etc/passwd"), context: Self.user)
      expectNoDifference(blocked.nodes.map(\.status), [.blocked])
      expectNoDifference(ui.opened, ["https://a.test/"])
      registration.unregister()
    }

    @Test func aSwiftExecutorRunsItsNodes() async throws {
      let (_, _, actions) = try await opened()
      let executor = RecordingExecutor()
      _ = try await actions.registerExecutor(for: "goto", executor)
      let goto = try PdfActionTree.decode(
        json: #"{"root":{"type":"goto","subtype":"GoTo","destination":{"kind":"fit","page":{"kind":"objectNumber","pageObjectNumber":1}},"next":[]},"incomplete":false,"warningFlags":0,"warnings":[]}"#
      )
      let result = try await actions.execute(goto, context: Self.user)
      expectNoDifference(result.nodes.map(\.status), [.executed])
      expectNoDifference(executor.types, ["goto"])
    }

    @Test func prepareCloseFindsTheWillCloseScript() async throws {
      let (_, _, actions) = try await opened()
      let result = try await actions.prepareClose()
      expectNoDifference(result.steps.count, 1)
      expectNoDifference(result.steps.first?.result.nodes.map(\.status), [.inert])
    }

    @Test func aDocumentVerbRunsItsOperation() async throws {
      let (_, _, actions) = try await opened()
      let ran = RecordingExecutor()
      try await actions.runDocumentVerb(.save) {
        try await Task.sleep(for: .milliseconds(5))
        ran.record("save")
      }
      expectNoDifference(ran.types, ["save"])
    }

    @Test func aDocumentVerbsThrownPDFErrorKeepsItsCode() async throws {
      let (_, _, actions) = try await opened()
      await #expect {
        try await actions.runDocumentVerb(.save) {
          throw PDFError(name: "PluginError", code: "permission-denied", detail: "no doc.download")
        }
      } throws: { error in
        (error as? PDFError)?.code == "permission-denied" && (error as? PDFError)?.detail == "no doc.download"
      }
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, actions) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await actions.policy()
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, actions) = try await opened()
      let task = Task { for await _ in actions.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await actions.prepareClose()
    }
  }
}

final class RecordingUI: ActionUiHandler, @unchecked Sendable {
  private let lock = NSLock()
  private var _opened: [String] = []
  var opened: [String] { lock.withLock { _opened } }

  func openUri(uri: String, options: ActionUriOptions) throws {
    lock.withLock { _opened.append(uri) }
  }
  func print(context: ActionUiContext?) throws {}
  func alert(message: String, options: ActionAlertOptions) throws {}
  func gotoPage(page: Double, context: ActionUiContext) throws {}
}

final class RecordingExecutor: ActionExecutor, @unchecked Sendable {
  private let lock = NSLock()
  private var _types: [String] = []
  var types: [String] { lock.withLock { _types } }
  func record(_ type: String) { lock.withLock { _types.append(type) } }

  func execute(node: PdfActionNode, context: ActionContext, completion: ActionExecutorCompletion) {
    record(node.nodeType())
    completion.executed()
  }
}
