import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct FormTests {
    func opened(_ fixture: String = "form_fields") async throws -> (PDFViewer, PDFDocument, FormCapability) {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, form: FormConfig(validation: FormValidation.none)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data(fixture))
      let form = try await viewer.form(for: document)
      try await form.refresh()
      return (viewer, document, form)
    }

    func writableText(_ form: FormCapability) async throws -> FormField {
      try #require(
        try await form.fields().first { field in
          if case .text = field.detail { return !field.flags.readOnly }
          return false
        })
    }

    @Test func fieldsListWithTypedDetail() async throws {
      let (_, _, form) = try await opened()
      let status = try await form.status()
      expectNoDifference(status, .ready)
      let fields = try await form.fields()
      #expect(!fields.isEmpty)
    }

    @Test func aTextFieldFillsAndIsObserved() async throws {
      let (_, _, form) = try await opened()
      let field = try await writableText(form)
      let events = form.events
      let result = try await form.setText("From Swift", for: field.fieldRef)
      expectNoDifference(result.status, .applied)
      for await event in events {
        guard case let .valueChanged(ref, _) = event else { continue }
        expectNoDifference(ref, field.fieldRef)
        break
      }
    }

    @Test func aCheckboxIsCheckedByItsOnState() async throws {
      let (_, _, form) = try await opened()
      let box = try #require(
        try await form.fields().first { if case .checkbox = $0.detail { return !$0.flags.readOnly } else { return false } })
      let onState = try #require(box.widgets.first?.toggle?.onState)
      // form_fields.pdf's box starts checked; clear it so checking is a change.
      let cleared = try await form.setChecked(nil, for: box.fieldRef)
      expectNoDifference(cleared.status, .applied)
      let result = try await form.setChecked(onState, for: box.fieldRef)
      expectNoDifference(result.status, .applied)
      let read = try #require(try await form.field(box.fieldRef))
      guard case let .checkbox(checked, _) = read.detail else { Issue.record("not a checkbox"); return }
      #expect(checked)
    }

    @Test func anXfdfExportRoundTrips() async throws {
      let (viewer, _, form) = try await opened()
      let field = try await writableText(form)
      _ = try await form.setText("Round trip", for: field.fieldRef)
      let export = try await form.exportData(.xfdf)
      expectNoDifference(export.format, .xfdf)
      let second = try await viewer.open(Fixture.data("form_fields"))
      let fresh = try await viewer.form(for: second)
      try await fresh.refresh()
      try await fresh.importData(export.bytes, format: .xfdf)
      let read = try #require(try await fresh.field(field.fieldRef))
      guard case let .text(value, _, _, _, _, _) = read.detail else { Issue.record("not text"); return }
      expectNoDifference(value, "Round trip")
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, form) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await form.fields()
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, form) = try await opened()
      let task = Task { for await _ in form.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await form.refresh()
    }

    @Test func refreshCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, form) = try await opened()
      let task = Task { try await form.refresh() }
      task.cancel()
      do {
        try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await form.fields()
    }

    @Test func exportCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, form) = try await opened()
      let task = Task { try await form.exportData(.fdf) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await form.exportData(.fdf)
    }

    // Cross-plugin (plan 15 merged first): a SubmitForm node reaches the
    // Swift submit handler through the form plugin's resolver, and a
    // document script runs in JavaScriptCore once forms and annotations
    // are reachable.

    final class RecordingSubmit: ActionSubmitHandler, @unchecked Sendable {
      private let lock = NSLock()
      private var urls: [String?] = []
      func submit(request: ActionSubmitRequest) throws { lock.withLock { urls.append(request.url) } }
      var seen: [String?] { lock.withLock { urls } }
    }

    @Test func aSubmitReachesTheSwiftHandler() async throws {
      let viewer = try PDFViewer(
        ViewerConfig(
          interaction: true, form: FormConfig(), actions: .forCurrentTimeZone(openSequence: .off)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("form_fields"))
      let actions = try await viewer.actions(for: document)
      let submit = RecordingSubmit()
      _ = try await actions.setSubmitHandler(submit)
      let tree = try PdfActionTree.decode(
        json: #"{"root":{"type":"submit-form","subtype":"SubmitForm","payload":{"url":"https://home.test/submit","fields":null,"flags":{"raw":4,"exclude":false,"includeNoValueFields":false,"format":"html","method":"post","submitCoordinates":false,"includeAppendSaves":false,"includeAnnotations":false,"canonicalFormat":false,"exclNonUserAnnots":false,"exclFKey":false,"embedForm":false}},"next":[]},"incomplete":false,"warningFlags":0,"warnings":[]}"#
      )
      _ = try await actions.execute(
        tree, context: ActionContext(origin: .user, source: .api, event: .activate))
      expectNoDifference(submit.seen, ["https://home.test/submit"])
    }

    @Test func aDocumentScriptRunsInJavaScriptCore() async throws {
      let viewer = try PDFViewer(
        ViewerConfig(
          interaction: true,
          annotation: AnnotationConfig(),
          form: FormConfig(),
          actions: .forCurrentTimeZone(
            openSequence: .off,
            javascript: ActionsJavaScriptConfig(enabled: true, engine: JavaScriptCoreEngineFactory()))))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("document_actions"))
      let actions = try await viewer.actions(for: document)
      let result = try await actions.prepareClose()
      // ActionsTests' `prepareCloseFindsTheWillCloseScript` sees `.inert`
      // with scripting off; with JavaScriptCore on and forms and
      // annotations reachable, the same script runs.
      expectNoDifference(result.steps.first?.result.nodes.map(\.status), [.executed])
    }
  }
}
