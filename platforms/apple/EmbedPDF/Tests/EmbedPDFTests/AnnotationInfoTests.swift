import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct AnnotationInfoTests {
    static let page = PageRef(pageObjectNumber: 1)
    static let ref = AnnotationRef.objectNumber(page: page, annotObjectNumber: 12)
    static let parent = AnnotationRef.objectNumber(page: page, annotObjectNumber: 7)

    static let inkJSON = ##"{"subtype":"ink","ref":{"kind":"objectNumber","page":{"kind":"objectNumber","objectNumber":1},"objectNumber":12},"rect":{"x":-1,"y":-1,"width":22,"height":7.000000000000001},"color":"#e5484d","opacity":0.5,"strokeWidth":2,"borderStyle":"dashed","dashArray":[3,1],"inkList":[[{"x":0,"y":0},{"x":10,"y":5}],[{"x":20,"y":0}]],"intent":"ink-highlight","rotation":30,"customMember":{"kept":true}}"##

    static func record(
      subtype: String = "ink", createdAt: String? = "2025-03-04T05:06:07Z",
      modifiedAt: String? = "2025-03-04T10:36:07+05:30", fieldsJSON: String = inkJSON
    ) -> AnnotationRecord {
      AnnotationRecord(
        annotationRef: ref, page: page, subtype: subtype,
        rect: Rect(x: -1, y: -1, width: 22, height: 7.000000000000001), contents: "Ink",
        author: "Reviewer", createdAt: createdAt, modifiedAt: modifiedAt, groupId: "group-1",
        inReplyTo: parent, canUpdate: true, canDelete: false, fieldsJson: fieldsJSON)
    }

    /// The instant `text` reads as through `AnnotationInfo`.
    static func instant(_ text: String?) -> Date? {
      AnnotationInfo(record(createdAt: text)).createdAt
    }

    @Test func everyMemberMapsFromTheGeneratedRecord() throws {
      let info = AnnotationInfo(Self.record())
      expectNoDifference(info.annotationRef, Self.ref)
      expectNoDifference(info.page, Self.page)
      expectNoDifference(info.subtype, .ink)
      expectNoDifference(info.rect, Rect(x: -1, y: -1, width: 22, height: 7.000000000000001))
      expectNoDifference(info.contents, "Ink")
      expectNoDifference(info.author, "Reviewer")
      expectNoDifference(info.createdAt, Date(timeIntervalSince1970: 1_741_064_767))
      expectNoDifference(info.rawCreatedAt, "2025-03-04T05:06:07Z")
      expectNoDifference(info.modifiedAt, Date(timeIntervalSince1970: 1_741_064_767))
      expectNoDifference(info.rawModifiedAt, "2025-03-04T10:36:07+05:30")
      expectNoDifference(info.groupID, "group-1")
      expectNoDifference(info.inReplyTo, Self.parent)
      expectNoDifference(info.canUpdate, true)
      expectNoDifference(info.canDelete, false)
      expectNoDifference(info.fieldsJSON, Self.inkJSON)
      expectNoDifference(info.highlight, nil)
      expectNoDifference(info.note, nil)
      let ink = try #require(info.ink)
      expectNoDifference(ink.color, "#e5484d")
      expectNoDifference(ink.opacity, 0.5)
      expectNoDifference(ink.strokeWidth, 2)
      expectNoDifference(ink.borderStyle, "dashed")
      expectNoDifference(ink.dashArray, [3, 1])
      expectNoDifference(
        ink.inkList, [[Point(x: 0, y: 0), Point(x: 10, y: 5)], [Point(x: 20, y: 0)]])
      expectNoDifference(ink.intent, "ink-highlight")
      expectNoDifference(ink.rotation, 30)
    }

    @Test func inkWithoutAColorIsStillInk() throws {
      // `/C` left out or empty reads as `"color": null`, and the other
      // optional members as `null` too.
      let json = ##"{"subtype":"ink","color":null,"opacity":1,"strokeWidth":1,"borderStyle":"solid","dashArray":null,"inkList":[[{"x":1,"y":2}]],"intent":null,"rotation":null}"##
      let record = Self.record(fieldsJSON: json)
      let info = AnnotationInfo(record)
      let ink = try #require(info.ink)
      #expect(ink.color == nil)
      #expect(ink.dashArray == nil)
      #expect(ink.intent == nil)
      #expect(ink.rotation == nil)
      expectNoDifference(ink.inkList, [[Point(x: 1, y: 2)]])
      expectNoDifference(info.generated, record)
      expectNoDifference(AnnotationInfo(info.generated), info)
    }

    @Test func aHighlightsQuadsAreTyped() {
      let json = ##"{"subtype":"highlight","color":"#ffcd45","opacity":1,"quadPoints":[{"upperLeft":{"x":10,"y":10},"upperRight":{"x":110,"y":10},"lowerLeft":{"x":10,"y":22},"lowerRight":{"x":110,"y":22}}]}"##
      let info = AnnotationInfo(Self.record(subtype: "highlight", fieldsJSON: json))
      expectNoDifference(info.highlight?.color, "#ffcd45")
      expectNoDifference(info.highlight?.opacity, 1)
      expectNoDifference(
        info.highlight?.quadPoints,
        [
          Quad(
            upperLeft: Point(x: 10, y: 10), upperRight: Point(x: 110, y: 10),
            lowerLeft: Point(x: 10, y: 22), lowerRight: Point(x: 110, y: 22))
        ])
      expectNoDifference(info.ink, nil)
    }

    @Test func aTextAnnotationIsANote() {
      let json = ##"{"subtype":"text","color":"#e5484d","opacity":1,"icon":"key","open":true,"state":"accepted","stateModel":"review"}"##
      let info = AnnotationInfo(Self.record(subtype: "text", fieldsJSON: json))
      expectNoDifference(info.subtype, .text)
      expectNoDifference(info.note?.color, "#e5484d")
      expectNoDifference(info.note?.opacity, 1)
      expectNoDifference(info.note?.icon, "key")
      expectNoDifference(info.note?.open, true)
      expectNoDifference(info.note?.state, "accepted")
      expectNoDifference(info.note?.stateModel, "review")
    }

    @Test func membersOfAnotherShapeAreLeftInFieldsJSON() {
      // `inkList` is not a list of strokes, so the ink members don't read.
      let json = ##"{"subtype":"ink","color":"#e5484d","opacity":1,"strokeWidth":2,"borderStyle":"solid","inkList":"none"}"##
      let info = AnnotationInfo(Self.record(fieldsJSON: json))
      expectNoDifference(info.subtype, .ink)
      expectNoDifference(info.details, .untyped)
      expectNoDifference(info.fieldsJSON, json)
    }

    @Test func anUnknownSubtypeKeepsItsName() {
      let json = ##"{"subtype":"sparkle","glow":3}"##
      let info = AnnotationInfo(Self.record(subtype: "sparkle", fieldsJSON: json))
      expectNoDifference(info.subtype, "sparkle")
      expectNoDifference(info.details, .untyped)
      expectNoDifference(info.generated.subtype, "sparkle")
      expectNoDifference(info.generated, Self.record(subtype: "sparkle", fieldsJSON: json))
    }

    @Test func theRoundTripLosesNothing() {
      let records = [
        Self.record(),
        Self.record(createdAt: nil, modifiedAt: nil),
        Self.record(createdAt: "not a date", modifiedAt: "2025-02-29T00:00:00Z"),
        Self.record(subtype: "sparkle", fieldsJSON: "{}"),
        AnnotationRecord(
          annotationRef: Self.ref, page: Self.page, subtype: "", rect: Rect(x: 0, y: 0, width: 0, height: 0),
          contents: nil, author: nil, createdAt: nil, modifiedAt: nil, groupId: nil, inReplyTo: nil,
          canUpdate: false, canDelete: true, fieldsJson: ""),
      ]
      for record in records {
        let info = AnnotationInfo(record)
        expectNoDifference(info.generated, record)
        expectNoDifference(AnnotationInfo(info.generated), info)
      }
    }

    @Test func theEnginesTimestampFormsRead() {
      let instant = Date(timeIntervalSince1970: 1_741_064_767)
      // What `pdf_date_to_iso` writes: `Z`, an offset, or none (UTC).
      expectNoDifference(Self.instant("2025-03-04T05:06:07Z"), instant)
      expectNoDifference(Self.instant("2025-03-04T10:36:07+05:30"), instant)
      expectNoDifference(Self.instant("2025-03-04T00:06:07-05:00"), instant)
      expectNoDifference(Self.instant("2025-03-04T05:06:07"), instant)
      // What the engine takes in: a fraction, no seconds, an offset without its colon.
      expectNoDifference(Self.instant("2025-03-04T05:06:07.25Z"), Date(timeIntervalSince1970: 1_741_064_767.25))
      expectNoDifference(Self.instant("2025-03-04T05:06Z"), Date(timeIntervalSince1970: 1_741_064_760))
      expectNoDifference(Self.instant("2025-03-04T10:36:07+0530"), instant)
      expectNoDifference(Self.instant("2024-02-29T23:59:00Z"), Date(timeIntervalSince1970: 1_709_251_140))
      expectNoDifference(Self.instant("1970-01-01T00:00:00+01:00"), Date(timeIntervalSince1970: -3600))
    }

    @Test func aMalformedTimestampKeepsItsText() {
      for text in [
        "2025-02-29T00:00:00Z", "2025-03-04T24:00:00Z", "2025-13-01T00:00:00Z",
        "2025-03-04T05:06:07+24:00", "D:20250304050607Z", "2025-03-04T05:06:07z",
        "2025-03-04T05:06:07+5:30", "2025-03-04T05:06:07Z ", "2025-03-04", "yesterday", "",
      ] {
        let info = AnnotationInfo(Self.record(createdAt: text, modifiedAt: text))
        #expect(info.createdAt == nil, "\(text)")
        expectNoDifference(info.rawCreatedAt, text)
        #expect(info.modifiedAt == nil, "\(text)")
        expectNoDifference(info.rawModifiedAt, text)
        expectNoDifference(info.generated.createdAt, text)
      }
    }

    @Test func anAbsentTimestampIsNil() {
      let info = AnnotationInfo(Self.record(createdAt: nil, modifiedAt: nil))
      #expect(info.createdAt == nil)
      #expect(info.rawCreatedAt == nil)
      #expect(info.modifiedAt == nil)
      #expect(info.rawModifiedAt == nil)
    }

    @Test func createdAnnotationsReadBackTyped() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, annotation: ""))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      let annotations = try await viewer.annotations(for: document)
      try await annotations.refresh()
      let events = annotations.events

      let ink = try await annotations.create(
        InkAnnotationDraft(
          inkList: [[Point(x: 100, y: 100), Point(x: 140, y: 120), Point(x: 180, y: 100)]],
          color: "#1f6feb", opacity: 0.75, strokeWidth: 3),
        on: page)
      let highlight = try await annotations.create(
        HighlightAnnotationDraft(
          quadPoints: [
            Quad(
              upperLeft: Point(x: 72, y: 300), upperRight: Point(x: 200, y: 300),
              lowerLeft: Point(x: 72, y: 312), lowerRight: Point(x: 200, y: 312))
          ],
          color: "#ffcd45"),
        on: page)
      let note = try await annotations.create(
        NoteAnnotationDraft(
          rect: Rect(x: 40, y: 40, width: 24, height: 24), icon: .key, contents: "Typed note"),
        on: page)

      let readInk = try #require(try await annotations.get(ink))
      expectNoDifference(readInk.subtype, .ink)
      let inkMembers = try #require(readInk.ink)
      expectNoDifference(
        inkMembers.inkList, [[Point(x: 100, y: 100), Point(x: 140, y: 120), Point(x: 180, y: 100)]])
      expectNoDifference(inkMembers.color, "#1f6feb")
      expectNoDifference(inkMembers.opacity, 0.75)
      expectNoDifference(inkMembers.strokeWidth, 3)

      let readHighlight = try #require(try await annotations.get(highlight))
      expectNoDifference(readHighlight.subtype, .highlight)
      let highlightMembers = try #require(readHighlight.highlight)
      expectNoDifference(highlightMembers.color, "#ffcd45")
      expectNoDifference(highlightMembers.quadPoints.count, 1)

      let readNote = try #require(try await annotations.get(note))
      expectNoDifference(readNote.subtype, .text)
      expectNoDifference(readNote.contents, "Typed note")
      expectNoDifference(readNote.note?.icon, "key")
      // The engine stamps a created annotation itself, in UTC with whole
      // seconds, and the stamp reads as the instant it names.
      let stamped = try #require(readNote.rawCreatedAt)
      let createdAt = try #require(readNote.createdAt, "the engine wrote \(stamped)")
      expectNoDifference(ISO8601DateFormatter().string(from: createdAt), stamped)
      #expect(abs(createdAt.timeIntervalSinceNow) < 300)
      if let modified = readNote.rawModifiedAt {
        #expect(readNote.modifiedAt != nil, "the engine wrote \(modified)")
      }

      // Every read spelling agrees, and each record round-trips.
      let listed = try await annotations.list(AnnotationFilter(pages: [page]))
      for created in [readInk, readHighlight, readNote] {
        #expect(listed.contains(created))
        expectNoDifference(AnnotationInfo(created.generated), created)
      }
      try await annotations.selection.select([note])
      let selected = try await annotations.selected()
      expectNoDifference(selected, [readNote])

      var createdSubtypes: [AnnotationSubtype] = []
      for await event in events {
        if case let .created(event) = event { createdSubtypes.append(event.annotationInfo.subtype) }
        if createdSubtypes.count == 3 { break }
      }
      expectNoDifference(createdSubtypes, [.ink, .highlight, .text])
    }

    @available(*, deprecated)
    @Test func theGeneratedRecordSpellingsStillRead() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, annotation: ""))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("annotation_kinds"))
      let annotations = try await viewer.annotations(for: document)
      try await annotations.refresh()
      let infos: [AnnotationInfo] = try await annotations.list()
      let records: [AnnotationRecord] = try await annotations.list()
      expectNoDifference(records, infos.map(\.generated))
      let first = try #require(infos.first)
      let record: AnnotationRecord? = try await annotations.get(first.annotationRef)
      expectNoDifference(record, first.generated)
      expectNoDifference(first.fieldsJson, record?.fieldsJson)
      expectNoDifference(first.groupId, record?.groupId)
      try await annotations.selection.select([first.annotationRef])
      let selected: [AnnotationRecord] = try await annotations.selected()
      expectNoDifference(selected, [first.generated])
      let center = Point(x: first.rect.x + first.rect.width / 2, y: first.rect.y + first.rect.height / 2)
      let hitInfo: AnnotationInfo? = try await annotations.hitTest(at: center, on: first.page)
      let hitRecord: AnnotationRecord? = try await annotations.hitTest(at: center, on: first.page)
      expectNoDifference(hitRecord, hitInfo?.generated)
    }
  }
}
