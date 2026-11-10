import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct EditingWorkflowsTests {
    func editingViewer() async throws -> PDFViewer {
      let viewer = try PDFViewer(ViewerConfig(
        interaction: true, annotation: "", form: FormConfig()))
      try await viewer.start()
      return viewer
    }

    func object(_ json: String) throws -> [String: Any] {
      try #require(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
    }

    func json(_ object: [String: Any]) throws -> String {
      String(decoding: try JSONSerialization.data(withJSONObject: object), as: UTF8.self)
    }

    @Test func commandControlsRespectCategoriesAndReportPermanentRefusals() async throws {
      let viewer = try PDFViewer(ViewerConfig(commands: true))
      try await viewer.start()
      let commands = try await viewer.commands()
      let runs = LockIsolated(0)
      _ = try await commands.register(CommandDefinition(
        id: "edit:note", labelKey: "Note", categories: ["edit.annotations"],
        handler: CommandHandler(run: { _ in runs.withValue { $0 += 1 } })))
      try await commands.disableCommand("edit:note")
      try await commands.disableCommand("edit:note")
      let disabledCommands = try await commands.disabledCommands()
      expectNoDifference(disabledCommands, ["edit:note"])
      let refusalReason = try await commands.refusal("edit:note")?.reason
      expectNoDifference(refusalReason, .turnedOff)
      let refusedExecution = try await commands.execute("edit:note")
      expectNoDifference(refusedExecution, .rejected(reason: .hidden))
      expectNoDifference(runs.value, 0)
      try await commands.disableCategory("edit")
      try await commands.enableCommand("edit:note")
      #expect(try await commands.isCommandDisabled("edit:note"))
      try await commands.enableCategory("edit")
      let allowedExecution = try await commands.execute("edit:note")
      expectNoDifference(allowedExecution, .executed)
      expectNoDifference(runs.value, 1)
      try await commands.setDisabledCommands(["", "missing"])
      #expect(try await commands.isCommandDisabled(""))
      #expect(try await commands.refusal("missing") == nil)
      try await commands.setDisabledCommands([])
      try await viewer.destroy()
      await #expect { try await commands.disabledCommands() } throws: { ($0 as? PDFError)?.code == .instanceClosed }
    }

    @Test func namedActionProbeReflectsPolicyAndClosedDocuments() async throws {
      let viewer = try PDFViewer(ViewerConfig(actions: ActionsConfig(openSequence: .off)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let actions = try await viewer.actions(for: document)
      #expect(try await actions.canExecuteNamed("NextPage"))
      try await actions.updatePolicy(ActionPolicyPatch(named: PartialActionPolicyRow(user: .block)))
      #expect(try await !actions.canExecuteNamed("NextPage"))
      try await viewer.close(document)
      await #expect { try await actions.canExecuteNamed("NextPage") } throws: { ($0 as? PDFError)?.code == .instanceClosed }
    }

    @Test func annotationBundlesPreserveOwnedResourcesAndTypedMappings() async throws {
      let viewer = try await editingViewer()
      let first = try await viewer.open(Fixture.data("report"))
      let second = try await viewer.open(Fixture.data("report"))
      let source = try await viewer.annotations(for: first)
      let target = try await viewer.annotations(for: second)
      let page = try await viewer.pages(of: first)[0].page
      let bytes = Data([0, 255, 128, 0, 239])
      let file = try await source.drafts.createAttachment(
        AnnotationAttachmentFile(bytes: bytes, name: "résumé.bin"), at: Point(x: 40, y: 40), on: page)
      let square = try await source.create(
        #"{"subtype":"square","box":{"x":80,"y":40,"width":40,"height":30}}"#, on: page)
      let bundle = try await source.export(refs: [file, square])
      #expect(!bundle.contains("$host"))
      let exported = try object(bundle)
      let resources = try #require(exported["resources"] as? [String: [String: String]])
      #expect(resources.values.contains { Data(base64Encoded: $0["$bytes"] ?? "") == bytes })
      let copied = try await target.import(bundle, optionsJSON: #"{"pages":"by-position","attribution":"stamp"}"#)
      expectNoDifference(copied.annotations.count, 2)
      #expect(copied.refMap.contains { $0.from == file })
      for record in copied.annotations {
        #expect(try await target.get(record.annotationRef) != nil)
      }
      let again = try object(try await target.export(refs: copied.annotations.map(\.annotationRef)))
      expectNoDifference(again["resources"] as? [String: [String: String]], resources)
      try await source.selection.select([square])
      let result = try await source.selection.updateLink(.uri(uri: "https://example.test/selected"))
      #expect(result.failed.isEmpty)
      #expect(try await source.threads.link(square) != nil)
      _ = try await source.selection.updateLink(nil)
      #expect(try await source.threads.link(square) == nil)
      let savedBytes = try await viewer.save(second)
      let saved = try await viewer.open(savedBytes)
      let reopened = try await viewer.annotations(for: saved)
      let persisted = try object(try await reopened.export(refs: copied.annotations.map(\.annotationRef)))
      expectNoDifference(persisted["resources"] as? [String: [String: String]], resources)
      try await viewer.close(first)
      await #expect { try await source.export() } throws: { ($0 as? PDFError)?.code == .instanceClosed }
      #expect(try await target.get(copied.annotations[0].annotationRef) != nil)
    }

    @Test func independentWidgetEditsAndCalculationOrderPersist() async throws {
      let viewer = try await editingViewer()
      let document = try await viewer.open(Fixture.data("report"))
      let form = try await viewer.form(for: document)
      let page = try await viewer.pages(of: document)[0].page
      let first = try await form.create(FormCreateFieldInput(
        family: .text, name: "first", page: page, bounds: Rect(x: 20, y: 30, width: 100, height: 24)))
      let second = try await form.create(FormCreateFieldInput(
        family: .text, name: "second", page: page, bounds: Rect(x: 20, y: 70, width: 100, height: 24)))
      var bundle = try object(try await form.export([first.fieldRef, second.fieldRef]))
      var fields = try #require(bundle["fields"] as? [[String: Any]])
      var refs: [Any] = []
      for index in fields.indices {
        var data = try #require(fields[index]["data"] as? [String: Any])
        refs.append(try #require(data["ref"]))
        data["name"] = "copy_\(index)"
        data["actions"] = ["calculate": ["root": ["type": "javascript", "subtype": "JavaScript", "script": "event.value = 1", "next": []] as [String: Any], "incomplete": false, "warnings": []] as [String: Any]]
        fields[index]["data"] = data
      }
      bundle["fields"] = fields
      bundle["calculationOrder"] = refs
      let imported = try await form.import(json(bundle))
      expectNoDifference(imported.fields.count, 2)
      let a = imported.fields[0]
      let b = imported.fields[1]
      let widget = try #require(a.widgets[0].annotationRef)
      let interaction = try await viewer.interaction(for: document)
      let annotations = try await viewer.annotations(for: document)
      try await interaction.activateTool("form-edit")
      try await annotations.selection.select([widget])
      let selectedField = try await form.selectedField()?.fieldRef
      expectNoDifference(selectedField, a.fieldRef)
      try await annotations.selection.clear()
      let updated = try await form.updateWidget(widget, #"{"rect":{"x":60,"y":90,"width":160,"height":28}}"#)
      expectNoDifference(updated.bounds.x, 60)
      let widgetRefs = try await form.widgets(of: a.fieldRef).map(\.annotationRef)
      expectNoDifference(widgetRefs, [widget])
      let extra = try await form.addWidget(to: a.fieldRef, FormWidgetPlacement(page: page, bounds: Rect(x: 20, y: 150, width: 100, height: 24)))
      let added = try #require(extra.field.widgets.last?.annotationRef)
      let remainingWidgetCount = try await form.deleteWidget(added)?.widgets.count
      expectNoDifference(remainingWidgetCount, 1)
      #expect(try await form.widget(added) == nil)
      let order = try await form.reorderCalculations([b.fieldRef], to: .before(field: a.fieldRef))
      expectNoDifference(order, [b.fieldRef, a.fieldRef])
      let savedBytes = try await viewer.save(document)
      let saved = try await viewer.open(savedBytes)
      let reopened = try await viewer.form(for: saved)
      try await reopened.refresh()
      let reopenedOrder = try await reopened.snapshot()?.calculationOrder
      expectNoDifference(reopenedOrder, order)
      let reopenedWidgetX = try await reopened.widget(widget)?.bounds.x
      expectNoDifference(reopenedWidgetX, 60)
      try await viewer.close(document)
      await #expect { try await form.selectedField() } throws: { ($0 as? PDFError)?.code == .instanceClosed }
      await #expect { try await form.deleteWidget(widget) } throws: { ($0 as? PDFError)?.code == .instanceClosed }
    }

    @Test func directFormPayloadsAuthorOptionsScriptsAndPersistNullEdits() async throws {
      let viewer = try await editingViewer()
      let document = try await viewer.open(Fixture.data("report"))
      let form = try await viewer.form(for: document)
      try await form.refresh()
      let page = try #require(try await viewer.pages(of: document).first).page
      let action: [String: Any] = ["type": "javascript", "script": "event.value = 7", "next": []]
      let field = try await form.create(draftJSON: json([
        "kind": "combobox", "name": "direct_native", "edit": true,
        "alternateName": "Preserved", "mappingName": "Clear", "defaultValue": "a",
        "options": [["label": "Alpha", "value": "a"], ["label": "Beta", "value": "b"]],
        "actions": ["calculate": action],
        "widgets": [["page": ["kind": "objectNumber", "objectNumber": page.pageObjectNumber],
                     "rect": ["x": 20, "y": 30, "width": 120, "height": 24]]]
      ]))
      if case let .combobox(_, _, edit, options) = field.detail {
        #expect(edit)
        expectNoDifference(options.map(\.value), ["a", "b"])
      } else { Issue.record("Expected combobox") }
      let updated = try await form.update(field.fieldRef, patchJSON: json([
        "mappingName": NSNull(), "edit": false, "actions": ["validate": action]
      ]))
      expectNoDifference(updated.alternateName, "Preserved")
      #expect(updated.mappingName == nil)
      let panel = try #require(try await form.fieldSettings(field.fieldRef))
      let choice = try #require(panel.properties.first { $0.key == "defaultValue" })
      expectNoDifference(choice.control, .choice)
      expectNoDifference(choice.free, false)
      expectNoDifference(choice.options, ["a", "b"])
      #expect(try object(panel.valuesJson)["edit"] as? Bool == false)
      let added = try await form.addWidget(to: field.fieldRef, placementJSON: json([
        "rect": ["x": 20, "y": 70, "width": 120, "height": 24],
        "fontSize": 19, "actions": ["focus": action]
      ]))
      #expect(try object(added.widgetJson)["fontSize"] as? Double == 19)
      let count = try await form.fields().count
      for malformed in ["{", "[]", "null", "42"] {
        await #expect { try await form.create(draftJSON: malformed) } throws: { ($0 as? PDFError)?.code == .invalidInput }
        await #expect { try await form.update(field.fieldRef, patchJSON: malformed) } throws: { ($0 as? PDFError)?.code == .invalidInput }
        await #expect { try await form.addWidget(to: field.fieldRef, placementJSON: malformed) } throws: { ($0 as? PDFError)?.code == .invalidInput }
      }
      let currentFieldCount = try await form.fields().count
      expectNoDifference(currentFieldCount, count)
      let saved = try await viewer.open(try await viewer.save(document))
      let reopened = try await viewer.form(for: saved)
      try await reopened.refresh()
      let found = try #require(try await reopened.field(field.fieldRef))
      expectNoDifference(found.alternateName, "Preserved")
      #expect(found.mappingName == nil)
      let actions = try #require(try object(found.fieldsJson)["actions"] as? [String: Any])
      let validate = try #require(actions["validate"] as? [String: Any])
      let root = try #require(validate["root"] as? [String: Any])
      expectNoDifference(root["script"] as? String, "event.value = 7")
      let reopenedWidgetCount = try await reopened.widgets(of: found.fieldRef).count
      expectNoDifference(reopenedWidgetCount, 2)
      try await viewer.close(document)
      await #expect { try await form.fieldSettings(field.fieldRef) } throws: { ($0 as? PDFError)?.code == .instanceClosed }
    }

    @Test func directFormPayloadsRespectScriptPermission() async throws {
      let viewer = try PDFViewer(ViewerConfig(scope: ["doc.open", "doc.forms.read", "doc.forms.modify"], interaction: true, form: FormConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let form = try await viewer.form(for: document)
      let actions = #"{"calculate":{"type":"javascript","script":"event.value = 1"}}"#
      await #expect { try await form.create(draftJSON: "{\"kind\":\"text\",\"name\":\"denied\",\"widgets\":[],\"actions\":" + actions + "}") } throws: { ($0 as? PDFError)?.code == .permissionDenied }
      let missing = FormFieldRef.fqn(name: "missing")
      await #expect { try await form.update(missing, patchJSON: "{\"actions\":" + actions + "}") } throws: { ($0 as? PDFError)?.code == .permissionDenied }
      await #expect { try await form.addWidget(to: missing, placementJSON: "{\"actions\":" + actions + "}") } throws: { ($0 as? PDFError)?.code == .permissionDenied }
    }

    @Test func optionalEditingPluginsReportUnsupported() async throws {
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      await #expect { try await viewer.annotations(for: document) } throws: { ($0 as? PDFError)?.code == .unsupported }
      await #expect { try await viewer.form(for: document) } throws: { ($0 as? PDFError)?.code == .unsupported }
      await #expect { try await viewer.actions(for: document) } throws: { ($0 as? PDFError)?.code == .unsupported }
      await #expect { try await viewer.commands() } throws: { ($0 as? PDFError)?.code == .unsupported }
    }
  }
}
