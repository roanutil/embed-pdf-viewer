import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct FormTests {
    func opened(_ fixture: String = "form_fields") async throws -> (PDFViewer, DocumentInfo, FormCapability) {
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
          if case .text = field.detail { return !field.readOnly }
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
      let result = try await form.setValue(.value(value: "From Swift"), for: field.fieldRef)
      expectNoDifference(result.status, .applied)
      for await event in events {
        guard case let .valueChanged(changed, _) = event else { continue }
        expectNoDifference(changed.fieldRef, field.fieldRef)
        break
      }
    }

    @Test func aCheckboxIsCheckedByItsExportValue() async throws {
      let (_, _, form) = try await opened()
      let box = try #require(
        try await form.fields().first { if case .checkbox = $0.detail { return !$0.readOnly } else { return false } })
      let exportValue = try #require(box.widgets.first?.toggle?.exportValue)
      // form_fields.pdf's box starts checked; clear it so checking is a change.
      let cleared = try await form.setValue(.value(value: nil), for: box.fieldRef)
      expectNoDifference(cleared.status, .applied)
      let result = try await form.setValue(.value(value: exportValue), for: box.fieldRef)
      expectNoDifference(result.status, .applied)
      let read = try #require(try await form.field(box.fieldRef))
      guard case let .checkbox(checked, _) = read.detail else { Issue.record("not a checkbox"); return }
      #expect(checked)
    }

    @Test func exportedValuesRoundTrip() async throws {
      let (viewer, _, form) = try await opened()
      let field = try await writableText(form)
      _ = try await form.setValue(.value(value: "Round trip"), for: field.fieldRef)
      let exported = try await form.exportValues()
      // The text field's value, by its full name. The whole export also
      // names `inline.direct`, a field with no object number, whose write
      // is refused `not-found` and fails the batch, as the TypeScript
      // engine refuses it (the `direct-field-refused` vector; the Rust
      // viewer-host test `a_form_bundle_export_fills_a_fresh_copy` does the same).
      let all = try #require(try JSONSerialization.jsonObject(with: Data(exported.utf8)) as? [String: Any])
      expectNoDifference(all[field.name] as? String, "Round trip")
      let values = String(
        decoding: try JSONSerialization.data(withJSONObject: [field.name: all[field.name]!]), as: UTF8.self)
      let second = try await viewer.open(Fixture.data("form_fields"))
      let fresh = try await viewer.form(for: second)
      try await fresh.refresh()
      let imported = try await fresh.importValues(values)
      #expect(imported.applied.contains(field.fieldRef))
      expectNoDifference(imported.failed, [])
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
      let task = Task { try await form.export() }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await form.export()
    }

    // One per member that takes a `CancelToken`, beyond `refresh` and
    // `export` above. Each is called in an already-cancelled task. Members
    // the re-ported plugin dropped are checked through what replaced them:
    // `setText`, `setChecked` and `setChoice` through `setValue`'s
    // `.value`, `.checked` and `.selectedValues`; `resetAll` through
    // `reset(nil)`; `createField`, `updateField` and `deleteField` through
    // `create`, `update` and `delete`; `detachWidget` and `attachWidget`
    // through `removeWidget` and `addWidget`; `importData` through the
    // bundle `import`.

    /// A writable text field, the first widget on the document, and its page.
    func cancellable() async throws -> (FormCapability, FormField, AnnotationRef, PageRef) {
      let (_, _, form) = try await opened()
      let field = try await writableText(form)
      let widget = try #require(field.widgets.first)
      return (form, field, try #require(widget.annotationRef), try #require(widget.page))
    }

    @Test func setValueCancelsWhenTheTaskIsCancelled() async throws {
      let (form, field, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await form.setValue(.value(value: "x"), for: field.fieldRef) }
      let value = try await form.value(of: field.fieldRef)
      #expect(value != .value(value: "x"))
    }

    @Test func setValueOfTextCancelsWhenTheTaskIsCancelled() async throws {
      let (form, field, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await form.setValue(.value(value: nil), for: field.fieldRef) }
    }

    @Test func setValueCheckedCancelsWhenTheTaskIsCancelled() async throws {
      let (form, field, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await form.setValue(.checked(checked: false), for: field.fieldRef) }
    }

    @Test func setValueSelectedValuesCancelsWhenTheTaskIsCancelled() async throws {
      let (form, field, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts {
        try await form.setValue(.selectedValues(values: ["a"]), for: field.fieldRef)
      }
    }

    @Test func setValuesCancelsWhenTheTaskIsCancelled() async throws {
      let (form, field, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts {
        try await form.setValues([FormValueEntry(fieldRef: field.fieldRef, value: .value(value: "x"))])
      }
    }

    @Test func importValuesCancelsWhenTheTaskIsCancelled() async throws {
      let (form, field, _, _) = try await cancellable()
      let values = String(decoding: try JSONEncoder().encode([field.name: "x"]), as: UTF8.self)
      await expectCancelledBeforeItStarts { try await form.importValues(values) }
    }

    @Test func resetCancelsWhenTheTaskIsCancelled() async throws {
      let (form, field, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await form.reset([field.fieldRef]) }
    }

    @Test func resetOfEveryFieldCancelsWhenTheTaskIsCancelled() async throws {
      let (form, _, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await form.reset() }
    }

    @Test func createCancelsWhenTheTaskIsCancelled() async throws {
      let (form, _, _, page) = try await cancellable()
      let before = try await form.fields().count
      await expectCancelledBeforeItStarts {
        try await form.create(
          FormCreateFieldInput(
            family: .text, name: "fromSwift", page: page, bounds: Rect(x: 10, y: 10, width: 80, height: 20)))
      }
      let after = try await form.fields().count
      expectNoDifference(after, before)
    }

    @Test func updateCancelsWhenTheTaskIsCancelled() async throws {
      let (form, field, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await form.update(field.fieldRef, FormFieldPatch(readOnly: true)) }
    }

    @Test func deleteCancelsWhenTheTaskIsCancelled() async throws {
      let (form, field, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await form.delete(field.fieldRef) }
      let kept = try await form.field(field.fieldRef)
      #expect(kept != nil)
    }

    @Test func removeWidgetCancelsWhenTheTaskIsCancelled() async throws {
      let (form, field, widget, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await form.removeWidget(widget, from: field.fieldRef) }
      let kept = try #require(try await form.field(field.fieldRef))
      expectNoDifference(kept.widgets.count, field.widgets.count)
    }

    @Test func addWidgetCancelsWhenTheTaskIsCancelled() async throws {
      let (form, field, _, page) = try await cancellable()
      await expectCancelledBeforeItStarts {
        try await form.addWidget(
          to: field.fieldRef, FormWidgetPlacement(page: page, bounds: Rect(x: 10, y: 10, width: 80, height: 20)))
      }
      let kept = try #require(try await form.field(field.fieldRef))
      expectNoDifference(kept.widgets.count, field.widgets.count)
    }

    @Test func importCancelsWhenTheTaskIsCancelled() async throws {
      let (form, _, _, _) = try await cancellable()
      let bundle = try await form.export()
      await expectCancelledBeforeItStarts { try await form.import(bundle) }
    }

    @Test func repairCancelsWhenTheTaskIsCancelled() async throws {
      let (form, _, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await form.repair() }
    }

    @Test func activateCancelsWhenTheTaskIsCancelled() async throws {
      let (form, _, widget, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await form.activate(widget) }
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
          annotation: "",
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
