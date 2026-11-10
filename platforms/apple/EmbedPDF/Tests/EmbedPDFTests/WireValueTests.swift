import CustomDump
import EmbedPDF
import Foundation
import Testing

/// Encode with `JSONEncoder`, decode the bytes back, and return both.
private func codableRoundTrip<Value: Codable>(_ value: Value) throws -> (json: String, decoded: Value) {
  let data = try JSONEncoder().encode(value)
  return (String(decoding: data, as: UTF8.self), try JSONDecoder().decode(Value.self, from: data))
}

extension BaseSuite {
  @Suite struct WireValueTests {
    // MARK: Open string sets

    static let subtypes: [(AnnotationSubtype, String)] = [
      (.text, "text"), (.link, "link"), (.freeText, "free-text"), (.line, "line"),
      (.square, "square"), (.circle, "circle"), (.polygon, "polygon"), (.polyline, "polyline"),
      (.highlight, "highlight"), (.underline, "underline"), (.squiggly, "squiggly"),
      (.strikeout, "strikeout"), (.stamp, "stamp"), (.caret, "caret"), (.ink, "ink"),
      (.popup, "popup"), (.fileAttachment, "file-attachment"), (.redact, "redact"),
      (.widget, "widget"), (.unsupported, "unsupported"),
    ]

    static let tools: [(ToolID, String)] = [
      (.pointer, "pointer"), (.pan, "pan"), (.zoom, "zoom"), (.snapshot, "snapshot"),
      (.formFill, "form-fill"), (.formEdit, "form-edit"), (.formText, "form-text"),
      (.formCheckbox, "form-checkbox"), (.formRadio, "form-radio"), (.formCombobox, "form-combobox"),
      (.formListbox, "form-listbox"), (.formButton, "form-button"), (.formSignature, "form-signature"),
      (.square, "square"), (.circle, "circle"), (.line, "line"), (.distance, "distance"),
      (.calibrate, "calibrate"), (.polygon, "polygon"), (.polyline, "polyline"),
      (.perimeter, "perimeter"), (.area, "area"), (.ink, "ink"), (.inkHighlight, "ink-highlight"),
      (.freeText, "free-text"), (.freeTextCallout, "free-text-callout"), (.highlight, "highlight"),
      (.redact, "redact"), (.underline, "underline"), (.strikeout, "strikeout"),
      (.squiggly, "squiggly"), (.insertText, "insert-text"), (.replaceText, "replace-text"),
      (.stamp, "stamp"), (.link, "link"), (.note, "note"), (.attachment, "attachment"),
    ]

    static let capabilities: [(DocumentPermission, String)] = [
      (.open, "doc.open"), (.render, "doc.render"), (.renderThumbnail, "doc.render.thumbnail"),
      (.textSelect, "doc.text.select"), (.textCopy, "doc.text.copy"),
      (.textSearch, "doc.text.search"), (.contentCopy, "doc.content.copy"),
      (.download, "doc.download"), (.downloadFlattened, "doc.download.flattened"),
      (.print, "doc.print"), (.printHigh, "doc.print.high"), (.pagesModify, "doc.pages.modify"),
      (.pagesAssemble, "doc.pages.assemble"), (.formsRead, "doc.forms.read"),
      (.formsFill, "doc.forms.fill"), (.formsModify, "doc.forms.modify"),
      (.formsImport, "doc.forms.import"), (.formsScript, "doc.forms.script"),
      (.formsSubmit, "doc.forms.submit"), (.annotateRead, "doc.annotate.read"),
      (.annotateModify, "doc.annotate.modify"), (.annotateImport, "doc.annotate.import"),
      (.metadataModify, "doc.metadata.modify"), (.attachmentsModify, "doc.attachments.modify"),
      (.outlineModify, "doc.outline.modify"), (.redact, "doc.redact"), (.sign, "doc.sign"),
      (.signCertify, "doc.sign.certify"), (.annotationsCreate, "annotations:create"),
    ]

    @Test func everyNamedCaseIsListed() {
      expectNoDifference(AnnotationSubtype.named.map(\.rawValue), Self.subtypes.map(\.1))
      expectNoDifference(ToolID.named.map(\.rawValue), Self.tools.map(\.1))
      expectNoDifference(DocumentPermission.named.map(\.rawValue), Self.capabilities.map(\.1))
    }

    @Test(arguments: subtypes) func eachSubtypeRoundTripsItsWireString(subtype: AnnotationSubtype, wire: String) throws {
      expectNoDifference(subtype.rawValue, wire)
      expectNoDifference(AnnotationSubtype(rawValue: wire), subtype)
      #expect(AnnotationSubtype(rawValue: wire) != AnnotationSubtype("\(wire)-x"))
      let (json, decoded) = try codableRoundTrip(subtype)
      expectNoDifference(json, "\"\(wire)\"")
      expectNoDifference(decoded, subtype)
    }

    @Test(arguments: tools) func eachToolRoundTripsItsWireString(tool: ToolID, wire: String) throws {
      expectNoDifference(tool.rawValue, wire)
      expectNoDifference(ToolID(rawValue: wire), tool)
      let (json, decoded) = try codableRoundTrip(tool)
      expectNoDifference(json, "\"\(wire)\"")
      expectNoDifference(decoded, tool)
    }

    @Test(arguments: capabilities) func eachCapabilityRoundTripsItsWireString(
      capability: DocumentPermission, wire: String
    ) throws {
      expectNoDifference(capability.rawValue, wire)
      expectNoDifference(DocumentPermission(rawValue: wire), capability)
      let (json, decoded) = try codableRoundTrip(capability)
      expectNoDifference(json, "\"\(wire)\"")
      expectNoDifference(decoded, capability)
    }

    @Test func anUnnamedStringRoundTripsAsItself() throws {
      let subtype = AnnotationSubtype(rawValue: "sound")
      expectNoDifference(subtype, "sound")
      expectNoDifference(subtype.rawValue, "sound")
      #expect(!AnnotationSubtype.named.contains(subtype))
      expectNoDifference(try codableRoundTrip(subtype).decoded, "sound")

      let tool = ToolID(rawValue: "laser")
      expectNoDifference(tool, "laser")
      expectNoDifference(tool.rawValue, "laser")
      #expect(!ToolID.named.contains(tool))
      expectNoDifference(try codableRoundTrip(tool).decoded, "laser")

      let permission = DocumentPermission(rawValue: "protected-display.view")
      expectNoDifference(permission, "protected-display.view")
      expectNoDifference(permission.rawValue, "protected-display.view")
      #expect(!DocumentPermission.named.contains(permission))
      expectNoDifference(try codableRoundTrip(permission).decoded, "protected-display.view")
    }

    @Test func aStringLiteralIsTheNamedConstant() {
      expectNoDifference("ink" as AnnotationSubtype, .ink)
      expectNoDifference("pan" as ToolID, .pan)
      expectNoDifference("doc.print" as DocumentPermission, .print)
      expectNoDifference(Set([ToolID("pan"), .pan]).count, 1)
    }

    @Test func aSwitchMatchesTheNamedConstantsAndFallsBackForTheRest() {
      func kind(_ tool: ToolID) -> String {
        switch tool {
        case .pan, .zoom: "camera"
        case .ink: "ink"
        default: "other"
        }
      }
      expectNoDifference(["pan", "zoom", "ink", "laser"].map { kind(ToolID($0)) }, ["camera", "camera", "ink", "other"])
    }

    @Test func descriptionIsTheWireString() {
      expectNoDifference("\(ToolID.inkHighlight)", "ink-highlight")
      expectNoDifference("\(AnnotationSubtype.fileAttachment)", "file-attachment")
      expectNoDifference("\(DocumentPermission.printHigh)", "doc.print.high")
    }

    // MARK: AnnotationPosition

    static let page = PageRef(pageObjectNumber: 3)
    static let positions: [(AnnotationPosition, String)] = [
      (.start, #""start""#),
      (.end, #""end""#),
      (
        .before(annotation: .objectNumber(page: page, annotObjectNumber: 7)),
        #"{"before":{"kind":"objectNumber","page":{"kind":"objectNumber","objectNumber":3},"objectNumber":7}}"#
      ),
      (
        .after(annotation: .objectNumber(page: page, annotObjectNumber: 7)),
        #"{"after":{"kind":"objectNumber","page":{"kind":"objectNumber","objectNumber":3},"objectNumber":7}}"#
      ),
      (
        .before(annotation: .index(page: page, index: 0)),
        #"{"before":{"kind":"baseIndex","page":{"kind":"objectNumber","objectNumber":3},"baseIndex":0}}"#
      ),
      (
        .after(annotation: .index(page: page, index: 4294967295)),
        #"{"after":{"kind":"baseIndex","page":{"kind":"objectNumber","objectNumber":3},"baseIndex":4294967295}}"#
      ),
    ]

    @Test(arguments: positions) func aPositionIsTheEnginesJSONByteForByte(position: AnnotationPosition, json: String) throws {
      expectNoDifference(Array(position.json.utf8), Array(json.utf8))
      expectNoDifference(try JSONDecoder().decode(AnnotationPosition.self, from: Data(json.utf8)), position)
      expectNoDifference(try codableRoundTrip(position).decoded, position)
    }

    @Test func aPositionThatIsNotOneRefusesToDecode() {
      for json in [#""front""#, #""above""#, #"{"above":{"kind":"objectNumber","page":{"kind":"objectNumber","objectNumber":3},"objectNumber":7}}"#] {
        #expect(throws: DecodingError.self) { try JSONDecoder().decode(AnnotationPosition.self, from: Data(json.utf8)) }
      }
    }

    // MARK: Against the engine

    /// A fresh copy of the fixture, and the refs on the page with the most
    /// annotations, in paint order.
    func crowdedPage() async throws -> (AnnotationCapability, [AnnotationRef]) {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, annotation: ""))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("annotation_kinds"))
      let annotations = try await viewer.annotations(for: document)
      try await annotations.refresh()
      let records = try await annotations.list()
      let pages = Dictionary(grouping: records, by: \.page)
      let page = try #require(pages.max { $0.value.count < $1.value.count }?.key)
      let refs = try await order(annotations, on: page)
      try #require(refs.count >= 3)
      return (annotations, refs)
    }

    func order(_ annotations: AnnotationCapability, on page: PageRef) async throws -> [AnnotationRef] {
      try await annotations.list(AnnotationFilter(pages: [page])).map(\.annotationRef)
    }

    func page(of ref: AnnotationRef) -> PageRef {
      switch ref {
      case let .objectNumber(page, _), let .index(page, _): page
      }
    }

    @Test func eachTypedPositionMovesTheAnnotationWhereItSays() async throws {
      let (annotations, original) = try await crowdedPage()
      let page = page(of: original[0])
      let (first, second, last) = (original[0], original[1], original[original.count - 1])

      try await annotations.reorder([first], to: .end)
      let top = try await order(annotations, on: page).last
      expectNoDifference(top, first)

      try await annotations.reorder([first], to: .start)
      let bottom = try await order(annotations, on: page).first
      expectNoDifference(bottom, first)

      try await annotations.reorder([first], to: .after(annotation: last))
      let afterLast = try await order(annotations, on: page)
      expectNoDifference(afterLast.last, first)

      try await annotations.reorder([first], to: .before(annotation: second))
      let beforeSecond = try await order(annotations, on: page)
      let index = try #require(beforeSecond.firstIndex(of: second))
      expectNoDifference(beforeSecond[index - 1], first)
    }

    @available(*, deprecated)
    @Test func eachTypedPositionReordersAsItsJSONStringDoes() async throws {
      for make in [
        { (_: [AnnotationRef]) in AnnotationPosition.start },
        { _ in .end },
        { refs in .before(annotation: refs[2]) },
        { refs in .after(annotation: refs[refs.count - 1]) },
      ] as [([AnnotationRef]) -> AnnotationPosition] {
        let (typed, refs) = try await crowdedPage()
        let position = make(refs)
        try await typed.reorder([refs[0]], to: position)
        let (stringly, sameRefs) = try await crowdedPage()
        expectNoDifference(sameRefs, refs)
        try await stringly.reorder([refs[0]], to: position.json)
        let page = page(of: refs[0])
        let typedOrder = try await order(typed, on: page)
        let stringOrder = try await order(stringly, on: page)
        expectNoDifference(typedOrder, stringOrder)
        #expect(typedOrder != refs || position == .start)
      }
    }

    /// The vocabulary the engine reads is `start`, `end`, `before` and
    /// `after`; `front` is not a position, and nothing moves.
    @available(*, deprecated)
    @Test func theEngineRefusesAPositionOutsideItsVocabulary() async throws {
      let (annotations, original) = try await crowdedPage()
      await #expect(throws: PDFError.self) { try await annotations.reorder([original[0]], to: #""front""#) }
      let unmoved = try await order(annotations, on: page(of: original[0]))
      expectNoDifference(unmoved, original)
    }

    @Test func everyBuiltInToolIsANamedCase() async throws {
      let viewer = try PDFViewer(
        ViewerConfig(render: RenderConfig(), interaction: true, selection: true, annotation: "", form: FormConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let interaction = try await viewer.interaction(for: document)
      let registered = try await interaction.tools().map { ToolID(rawValue: $0.id) }
      expectNoDifference(registered.filter { !ToolID.named.contains($0) }, [])
      expectNoDifference(Set(registered), Set(ToolID.named))
      for tool in ToolID.named {
        #expect(try await interaction.hasTool(tool), "\(tool.rawValue)")
      }
      #expect(try await !interaction.hasTool("laser"))
      try await interaction.activateTool(.formEdit)
      let active = try await interaction.activeToolId()
      expectNoDifference(active, "form-edit")

      let annotationTools = try await viewer.annotations(for: document).tools.tools()
      expectNoDifference(annotationTools.map { ToolID(rawValue: $0.id) }.filter { !ToolID.named.contains($0) }, [])
    }

    @Test func aTypedToolReachesTheAnnotationTools() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, annotation: ""))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let tools = try await viewer.annotations(for: document).tools
      let highlightID = try await tools.tool(.highlight)?.id
      expectNoDifference(highlightID, "highlight")
      try await tools.updateDefaults(##"{"color": "#00ff00"}"##, for: .highlight)
      #expect(try await tools.defaults(for: .highlight).contains("#00ff00"))
      _ = try await tools.properties(for: .highlight)
      let laserID = try await tools.tool("laser")?.id
      expectNoDifference(laserID, nil)
    }

    static let quads = #"[{"upperLeft":{"x":72,"y":72},"upperRight":{"x":172,"y":72},"lowerLeft":{"x":72,"y":92},"lowerRight":{"x":172,"y":92}}]"#
    static let anchor = AnnotationTextEndAnchor(
      anchorJson: #"{"glyphQuad":{"upperLeft":{"x":172,"y":72},"upperRight":{"x":178,"y":72},"lowerLeft":{"x":172,"y":92},"lowerRight":{"x":178,"y":92}},"advance":1}"#)
    static let square = #"{"subtype": "square", "box": {"x": 200, "y": 200, "width": 10, "height": 10}}"#

    /// The report, editable, with its text selectable and the first page's
    /// geometry loaded.
    func editable() async throws -> (PDFViewer, DocumentInfo, PageRef, AnnotationCapability, SelectionCapability) {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, selection: true, annotation: ""))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      let annotations = try await viewer.annotations(for: document)
      try await annotations.refresh()
      let selection = try await viewer.selection(for: document)
      try await selection.ensureLoaded(page)
      return (viewer, document, page, annotations, selection)
    }

    @Test func typedSubtypesAndToolsReachTheDrafts() async throws {
      let (_, _, page, annotations, selection) = try await editable()
      let drafts = annotations.drafts
      _ = try await annotations.create(Self.square, on: page, tool: .square)
      try await drafts.createMarkup(.underline, quadsJSON: Self.quads, on: page, preset: .underline)
      try await drafts.createReplaceText(quadsJSON: Self.quads, caretAt: Self.anchor, on: page, preset: .replaceText)
      try await drafts.previewMarkup(.highlight, pages: [AnnotationPageQuads(page: page, quadsJson: Self.quads)], preset: .highlight)
      try await drafts.clearMarkupPreview()
      try await drafts.hoverGhost(tool: .note, at: Point(x: 300, y: 300), on: page)
      try await drafts.clearGhost()
      try await drafts.previewPlacement(tool: .square, from: Point(x: 300, y: 300), to: Point(x: 340, y: 340), on: page)
      try await drafts.clearPlacementPreview()
      try await selection.selectPage(page)
      #expect(!(try await drafts.createFromSelection(tool: .highlight)).isEmpty)
      let subtypes = try await annotations.list().map(\.subtype)
      expectNoDifference(Set(subtypes), [.square, .underline, .strikeout, .caret, .highlight])
    }

    @Test func aTypedCapabilityAsksTheScope() async throws {
      let viewer = try PDFViewer(ViewerConfig(scope: ["doc.open", "doc.render"]))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      #expect(try await viewer.allows(.render, in: document))
      #expect(try await !viewer.allows(.download, in: document))
      #expect(try await !viewer.allows("no.such-capability", in: document))
    }

    @available(*, deprecated)
    @Test func theStringSpellingsStillWork() async throws {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, selection: true, annotation: ""))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      for (capability, wire) in Self.capabilities {
        let typed = try await viewer.allows(capability, in: document)
        let stringly = try await viewer.allows(wire, in: document)
        #expect(typed == stringly, "\(wire)")
      }
      #expect(try await viewer.allows("doc.render", in: document))

      let interaction = try await viewer.interaction(for: document)
      #expect(try await interaction.hasTool("pan"))
      let panID = try await interaction.tool("pan")?.id
      expectNoDifference(panID, "pan")
      try await interaction.activateTool("highlight")
      let activated = try await interaction.activeToolId()
      expectNoDifference(activated, "highlight")
      try await interaction.pushTool("pan")
      let pushed = try await interaction.activeToolId()
      expectNoDifference(pushed, "pan")
      try await interaction.setToolCursor("pan", cursors: nil)

      let annotations = try await viewer.annotations(for: document)
      try await annotations.refresh()
      let annotationHighlightID = try await annotations.tools.tool("highlight")?.id
      expectNoDifference(annotationHighlightID, "highlight")
      try await annotations.tools.updateDefaults(##"{"color": "#00ff00"}"##, for: "highlight")
      #expect(try await annotations.tools.defaults(for: "highlight").contains("#00ff00"))
      _ = try await annotations.tools.properties(for: "highlight")

      let page = try await viewer.pages(of: document)[0].page
      let drafts = annotations.drafts
      _ = try await annotations.create(Self.square, on: page, tool: "square")
      try await drafts.createMarkup("squiggly", quadsJSON: Self.quads, on: page, preset: "squiggly")
      try await drafts.createReplaceText(quadsJSON: Self.quads, caretAt: Self.anchor, on: page, preset: "replace-text")
      try await drafts.previewMarkup("highlight", pages: [AnnotationPageQuads(page: page, quadsJson: Self.quads)], preset: "highlight")
      try await drafts.clearMarkupPreview()
      try await drafts.hoverGhost(tool: "note", at: Point(x: 300, y: 300), on: page)
      try await drafts.clearGhost()
      try await drafts.previewPlacement(tool: "square", from: Point(x: 300, y: 300), to: Point(x: 340, y: 340), on: page)
      try await drafts.clearPlacementPreview()
      let selection = try await viewer.selection(for: document)
      try await selection.ensureLoaded(page)
      try await selection.selectPage(page)
      #expect(!(try await drafts.createFromSelection(tool: "highlight")).isEmpty)
      let subtypes = try await annotations.list().map(\.subtype.rawValue)
      expectNoDifference(Set(subtypes), ["square", "squiggly", "strikeout", "caret", "highlight"])
    }
  }
}
