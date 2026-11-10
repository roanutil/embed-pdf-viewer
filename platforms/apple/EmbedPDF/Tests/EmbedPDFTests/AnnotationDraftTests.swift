import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  /// The typed drafts and patches. Encoding tests compare exact bytes: the
  /// encoder sorts keys, so a draft is always the same string. Engine tests
  /// create, update and set defaults through the typed overloads and read
  /// the result back as `AnnotationInfo`.
  @Suite struct AnnotationDraftTests {
    static let stroke = [Point(x: 100, y: 100), Point(x: 140, y: 120), Point(x: 180, y: 100)]
    static let quad = Quad(
      upperLeft: Point(x: 72, y: 300), upperRight: Point(x: 200, y: 300),
      lowerLeft: Point(x: 72, y: 312), lowerRight: Point(x: 200, y: 312))
    static let noteRect = Rect(x: 40, y: 40, width: 24, height: 24)

    /// Every ink member set, none to its default.
    static let fullInk = InkAnnotationDraft(
      inkList: [stroke], color: "#1f6feb", opacity: 0.75, strokeWidth: 3, borderStyle: .dashed,
      dashArray: [4, 2], intent: .value(.inkHighlight), rotation: 30, contents: "Ink text")
    /// Every highlight member set, none to its default.
    static let fullHighlight = HighlightAnnotationDraft(
      quadPoints: [quad], color: "#ffcd45", opacity: 0.5, contents: "Highlight text")
    /// Every note member set, none to its default.
    static let fullNote = NoteAnnotationDraft(
      rect: noteRect, color: "#e5484d", opacity: 0.5, icon: .key, open: true, state: "accepted",
      stateModel: "review", contents: "Typed note")

    func opened() async throws -> (PageRef, AnnotationCapability) {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, annotation: ""))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      let annotations = try await viewer.annotations(for: document)
      try await annotations.refresh()
      return (page, annotations)
    }

    func object(_ json: String) throws -> [String: Any] {
      try #require(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
    }

    // ── exact encoding ──────────────────────────────────────────────────

    @Test func anInkDraftEncodesEveryMember() throws {
      expectNoDifference(
        try Self.fullInk.json(),
        ##"{"borderStyle":"dashed","color":"#1f6feb","contents":"Ink text","dashArray":[4,2],"inkList":[[{"x":100,"y":100},{"x":140,"y":120},{"x":180,"y":100}]],"intent":"ink-highlight","opacity":0.75,"rotation":30,"strokeWidth":3,"subtype":"ink"}"##
      )
    }

    @Test func aDraftLeavesUnsetMembersOut() throws {
      expectNoDifference(
        try InkAnnotationDraft(inkList: [[Point(x: 1.5, y: 2)]]).json(),
        ##"{"inkList":[[{"x":1.5,"y":2}]],"subtype":"ink"}"##)
      expectNoDifference(
        try HighlightAnnotationDraft(quadPoints: [Self.quad]).json(),
        ##"{"quadPoints":[{"lowerLeft":{"x":72,"y":312},"lowerRight":{"x":200,"y":312},"upperLeft":{"x":72,"y":300},"upperRight":{"x":200,"y":300}}],"subtype":"highlight"}"##
      )
      expectNoDifference(
        try NoteAnnotationDraft(rect: Self.noteRect).json(),
        ##"{"rect":{"height":24,"width":24,"x":40,"y":40},"subtype":"text"}"##)
    }

    @Test func anInkDraftWritesNullForNothing() throws {
      let draft = InkAnnotationDraft(
        inkList: [[Point(x: 0, y: 0)]], color: .null, dashArray: .null, intent: .null,
        rotation: .null, contents: .null)
      expectNoDifference(
        try draft.json(),
        ##"{"color":null,"contents":null,"dashArray":null,"inkList":[[{"x":0,"y":0}]],"intent":null,"rotation":null,"subtype":"ink"}"##
      )
    }

    @Test func aHighlightDraftEncodesEveryMember() throws {
      expectNoDifference(
        try Self.fullHighlight.json(),
        ##"{"color":"#ffcd45","contents":"Highlight text","opacity":0.5,"quadPoints":[{"lowerLeft":{"x":72,"y":312},"lowerRight":{"x":200,"y":312},"upperLeft":{"x":72,"y":300},"upperRight":{"x":200,"y":300}}],"subtype":"highlight"}"##
      )
    }

    @Test func aNoteDraftEncodesEveryMember() throws {
      expectNoDifference(
        try Self.fullNote.json(),
        ##"{"color":"#e5484d","contents":"Typed note","icon":"key","opacity":0.5,"open":true,"rect":{"height":24,"width":24,"x":40,"y":40},"state":"accepted","stateModel":"review","subtype":"text"}"##
      )
      expectNoDifference(
        try NoteAnnotationDraft(rect: Self.noteRect, state: .null, stateModel: .null, contents: .null)
          .json(),
        ##"{"contents":null,"rect":{"height":24,"width":24,"x":40,"y":40},"state":null,"stateModel":null,"subtype":"text"}"##
      )
    }

    @Test func noteIconsAreTheEnginesNames() {
      expectNoDifference(
        AnnotationNoteIcon.allCases.map(\.rawValue),
        ["comment", "key", "note", "help", "new-paragraph", "paragraph", "insert"])
      expectNoDifference(AnnotationBorderStyle.allCases.map(\.rawValue), ["solid", "dashed"])
      expectNoDifference(AnnotationInkIntent.allCases.map(\.rawValue), ["ink-highlight"])
    }

    @Test func anEmptyPatchIsAnEmptyObject() throws {
      expectNoDifference(try InkAnnotationPatch().json(), "{}")
      expectNoDifference(try HighlightAnnotationPatch().json(), "{}")
      expectNoDifference(try NoteAnnotationPatch().json(), "{}")
    }

    @Test func anInkPatchEncodesEveryMemberAndNoSubtype() throws {
      let patch = InkAnnotationPatch(
        inkList: [[Point(x: 1, y: 2)], [Point(x: 3, y: 4)]], color: "#00ff00", opacity: 0.25,
        strokeWidth: 5, borderStyle: .solid, dashArray: [1], intent: .value(.inkHighlight),
        rotation: 90, contents: "Changed")
      expectNoDifference(
        try patch.json(),
        ##"{"borderStyle":"solid","color":"#00ff00","contents":"Changed","dashArray":[1],"inkList":[[{"x":1,"y":2}],[{"x":3,"y":4}]],"intent":"ink-highlight","opacity":0.25,"rotation":90,"strokeWidth":5}"##
      )
    }

    @Test func aPatchTellsClearingFromLeavingAlone() throws {
      expectNoDifference(
        try InkAnnotationPatch(
          color: .null, dashArray: .null, intent: .null, rotation: .null, contents: .null
        ).json(),
        ##"{"color":null,"contents":null,"dashArray":null,"intent":null,"rotation":null}"##)
      expectNoDifference(
        try NoteAnnotationPatch(state: .null, stateModel: .null, contents: .null).json(),
        ##"{"contents":null,"state":null,"stateModel":null}"##)
      expectNoDifference(
        try HighlightAnnotationPatch(contents: .null).json(), ##"{"contents":null}"##)
    }

    @Test func aHighlightPatchEncodesEveryMember() throws {
      let patch = HighlightAnnotationPatch(
        quadPoints: [Self.quad], color: "#00ff00", opacity: 1, contents: "Changed")
      expectNoDifference(
        try patch.json(),
        ##"{"color":"#00ff00","contents":"Changed","opacity":1,"quadPoints":[{"lowerLeft":{"x":72,"y":312},"lowerRight":{"x":200,"y":312},"upperLeft":{"x":72,"y":300},"upperRight":{"x":200,"y":300}}]}"##
      )
    }

    @Test func aNotePatchEncodesEveryMember() throws {
      let patch = NoteAnnotationPatch(
        rect: Rect(x: 1, y: 2, width: 3, height: 4), color: "#00ff00", opacity: 0.5,
        icon: .newParagraph, open: false, state: "rejected", stateModel: "review",
        contents: "Changed")
      expectNoDifference(
        try patch.json(),
        ##"{"color":"#00ff00","contents":"Changed","icon":"new-paragraph","opacity":0.5,"open":false,"rect":{"height":4,"width":3,"x":1,"y":2},"state":"rejected","stateModel":"review"}"##
      )
    }

    @Test func aStringIsWrittenAsGiven() throws {
      expectNoDifference(
        try HighlightAnnotationPatch(contents: #"a/b "c" é"#).json(),
        ##"{"contents":"a/b \"c\" é"}"##)
    }

    @Test func aNumberJSONCantHoldIsInvalidInput() {
      #expect { try InkAnnotationPatch(opacity: .nan).json() } throws: {
        ($0 as? PDFError)?.code == .invalidInput
      }
      #expect { try InkAnnotationDraft(inkList: [[Point(x: .infinity, y: 0)]]).json() } throws: {
        ($0 as? PDFError)?.code == .invalidInput
      }
    }

    // ── the engine takes every typed member ─────────────────────────────

    @Test func createdDraftsReadBackEveryMember() async throws {
      let (page, annotations) = try await opened()

      let ink = try #require(try await annotations.get(annotations.create(Self.fullInk, on: page)))
      expectNoDifference(ink.subtype, .ink)
      expectNoDifference(ink.contents, "Ink text")
      expectNoDifference(
        ink.ink.map(InkSeen.init),
        InkSeen(
          color: "#1f6feb", opacity: 0.75, strokeWidth: 3, borderStyle: "dashed", dashArray: [4, 2],
          inkList: [Self.stroke], intent: "ink-highlight", rotation: 30))

      let highlight = try #require(
        try await annotations.get(annotations.create(Self.fullHighlight, on: page)))
      expectNoDifference(highlight.subtype, .highlight)
      expectNoDifference(highlight.contents, "Highlight text")
      expectNoDifference(
        highlight.highlight.map(HighlightSeen.init),
        HighlightSeen(color: "#ffcd45", opacity: 0.5, quadPoints: [Self.quad]))

      let note = try #require(try await annotations.get(annotations.create(Self.fullNote, on: page)))
      expectNoDifference(note.subtype, .text)
      expectNoDifference(note.rect, Self.noteRect)
      expectNoDifference(note.contents, "Typed note")
      expectNoDifference(
        note.note.map(NoteSeen.init),
        NoteSeen(
          color: "#e5484d", opacity: 0.5, icon: "key", open: true, state: "accepted",
          stateModel: "review"))
    }

    @Test func aNullColorDrawsAnInkWithNoStroke() async throws {
      let (page, annotations) = try await opened()
      let ref = try await annotations.create(
        InkAnnotationDraft(inkList: [Self.stroke], color: .null), on: page)
      let ink = try #require(try await annotations.get(ref)?.ink)
      #expect(ink.color == nil)
    }

    @Test func updatesChangeEveryMember() async throws {
      let (page, annotations) = try await opened()

      let inkRef = try await annotations.create(Self.fullInk, on: page)
      let strokes = [[Point(x: 50, y: 60), Point(x: 70, y: 80)], [Point(x: 90, y: 100)]]
      try await annotations.update(
        inkRef,
        InkAnnotationPatch(
          inkList: strokes, color: "#00ff00", opacity: 0.25, strokeWidth: 5, borderStyle: .dashed,
          dashArray: [1, 1], rotation: 45, contents: "Changed ink"))
      let ink = try #require(try await annotations.get(inkRef))
      expectNoDifference(ink.contents, "Changed ink")
      expectNoDifference(
        ink.ink.map(InkSeen.init),
        InkSeen(
          color: "#00ff00", opacity: 0.25, strokeWidth: 5, borderStyle: "dashed", dashArray: [1, 1],
          inkList: strokes, intent: "ink-highlight", rotation: 45))

      let highlightRef = try await annotations.create(Self.fullHighlight, on: page)
      let moved = Quad(
        upperLeft: Point(x: 80, y: 400), upperRight: Point(x: 160, y: 400),
        lowerLeft: Point(x: 80, y: 414), lowerRight: Point(x: 160, y: 414))
      try await annotations.update(
        highlightRef,
        HighlightAnnotationPatch(
          quadPoints: [moved], color: "#00ff00", opacity: 1, contents: "Changed highlight"))
      let highlight = try #require(try await annotations.get(highlightRef))
      expectNoDifference(highlight.contents, "Changed highlight")
      expectNoDifference(
        highlight.highlight.map(HighlightSeen.init),
        HighlightSeen(color: "#00ff00", opacity: 1, quadPoints: [moved]))

      let noteRef = try await annotations.create(Self.fullNote, on: page)
      let rect = Rect(x: 100, y: 120, width: 20, height: 20)
      try await annotations.update(
        noteRef,
        NoteAnnotationPatch(
          rect: rect, color: "#00ff00", opacity: 1, icon: .newParagraph, open: false,
          state: "completed", stateModel: "review", contents: "Changed note"))
      let note = try #require(try await annotations.get(noteRef))
      expectNoDifference(note.rect, rect)
      expectNoDifference(note.contents, "Changed note")
      expectNoDifference(
        note.note.map(NoteSeen.init),
        NoteSeen(
          color: "#00ff00", opacity: 1, icon: "new-paragraph", open: false, state: "completed",
          stateModel: "review"))
    }

    @Test func aPatchLeavesWhatItLeavesOutAndClearsWhatItNulls() async throws {
      let (page, annotations) = try await opened()

      let inkRef = try await annotations.create(Self.fullInk, on: page)
      try await annotations.update(inkRef, InkAnnotationPatch(opacity: 0.5))
      let kept = try #require(try await annotations.get(inkRef))
      expectNoDifference(kept.ink?.opacity, 0.5)
      expectNoDifference(kept.ink?.color, "#1f6feb")
      expectNoDifference(kept.ink?.dashArray, [4, 2])
      expectNoDifference(kept.contents, "Ink text")

      try await annotations.update(
        inkRef,
        InkAnnotationPatch(
          color: .null, borderStyle: .solid, dashArray: .null, intent: .null, rotation: .null,
          contents: .null))
      let cleared = try #require(try await annotations.get(inkRef))
      let ink = try #require(cleared.ink)
      #expect(ink.color == nil)
      #expect(ink.dashArray == nil)
      #expect(ink.intent == nil)
      #expect(ink.rotation == nil)
      #expect(cleared.contents == nil)

      let noteRef = try await annotations.create(Self.fullNote, on: page)
      try await annotations.update(
        noteRef, NoteAnnotationPatch(state: .null, stateModel: .null, contents: .null))
      let note = try #require(try await annotations.get(noteRef))
      #expect(note.note?.state == nil)
      #expect(note.note?.stateModel == nil)
      #expect(note.contents == nil)
      expectNoDifference(note.note?.icon, "key")

      let highlightRef = try await annotations.create(Self.fullHighlight, on: page)
      try await annotations.update(highlightRef, HighlightAnnotationPatch(contents: .null))
      let highlight = try #require(try await annotations.get(highlightRef))
      #expect(highlight.contents == nil)
      expectNoDifference(highlight.highlight?.color, "#ffcd45")
    }

    @Test func aPatchOfAnotherSubtypesFieldsIsRefused() async throws {
      let (page, annotations) = try await opened()
      let highlight = try await annotations.create(Self.fullHighlight, on: page)
      await #expect {
        try await annotations.update(highlight, InkAnnotationPatch(strokeWidth: 2))
      } throws: { ($0 as? PDFError)?.code == .invalidInput }
    }

    @Test func defaultsReachTheToolsNextAnnotation() async throws {
      let (page, annotations) = try await opened()
      let tools = annotations.tools

      try await tools.updateDefaults(
        InkAnnotationPatch(
          color: "#00ff00", opacity: 0.5, strokeWidth: 4, borderStyle: .dashed, dashArray: [2, 3],
          rotation: 15, contents: "Default ink"),
        for: .ink)
      let ink = try #require(
        try await annotations.get(
          annotations.create(InkAnnotationDraft(inkList: [Self.stroke]), on: page, tool: .ink)))
      expectNoDifference(ink.contents, "Default ink")
      expectNoDifference(
        ink.ink.map(InkSeen.init),
        InkSeen(
          color: "#00ff00", opacity: 0.5, strokeWidth: 4, borderStyle: "dashed", dashArray: [2, 3],
          inkList: [Self.stroke], intent: nil, rotation: 15))

      try await tools.updateDefaults(
        HighlightAnnotationPatch(color: "#00ff00", opacity: 0.5, contents: "Default highlight"),
        for: .highlight)
      let highlight = try #require(
        try await annotations.get(
          annotations.create(
            HighlightAnnotationDraft(quadPoints: [Self.quad]), on: page, tool: .highlight)))
      expectNoDifference(highlight.contents, "Default highlight")
      expectNoDifference(
        highlight.highlight.map(HighlightSeen.init),
        HighlightSeen(color: "#00ff00", opacity: 0.5, quadPoints: [Self.quad]))

      try await tools.updateDefaults(
        NoteAnnotationPatch(
          color: "#00ff00", opacity: 0.5, icon: .help, open: true, state: "marked",
          stateModel: "marked", contents: "Default note"),
        for: .note)
      let note = try #require(
        try await annotations.get(
          annotations.create(NoteAnnotationDraft(rect: Self.noteRect), on: page, tool: .note)))
      expectNoDifference(note.contents, "Default note")
      expectNoDifference(
        note.note.map(NoteSeen.init),
        NoteSeen(
          color: "#00ff00", opacity: 0.5, icon: "help", open: true, state: "marked",
          stateModel: "marked"))
    }

    @Test func aNullDefaultReachesTheToolsNextAnnotation() async throws {
      let (page, annotations) = try await opened()
      try await annotations.tools.updateDefaults(InkAnnotationPatch(color: .null), for: .ink)
      let defaults = try object(try await annotations.tools.defaults(for: .ink))
      #expect(defaults["color"] is NSNull)
      let ink = try #require(
        try await annotations.get(
          annotations.create(InkAnnotationDraft(inkList: [Self.stroke]), on: page, tool: .ink)))
      #expect(ink.ink?.color == nil)
    }

    // ── drift guard ─────────────────────────────────────────────────────

    /// The engine refuses a key its kind doesn't declare
    /// (`assert_declared_fields`), so a typed key the engine renamed or
    /// dropped makes the create above throw. This pins that refusal: if the
    /// engine started ignoring unknown keys instead, the guard below would be
    /// the only check left.
    @Test func theEngineRefusesAKeyItsKindDoesntDeclare() async throws {
      let (page, annotations) = try await opened()
      for json in [
        ##"{"subtype":"ink","inkList":[[{"x":1,"y":2}]],"colour":"#ff0000"}"##,
        ##"{"subtype":"highlight","quadPoints":[{"upperLeft":{"x":72,"y":300},"upperRight":{"x":200,"y":300},"lowerLeft":{"x":72,"y":312},"lowerRight":{"x":200,"y":312}}],"strokeWidth":2}"##,
        ##"{"subtype":"text","rect":{"x":40,"y":40,"width":24,"height":24},"stateModell":"review"}"##,
      ] {
        await #expect("\(json)") {
          try await annotations.create(json, on: page)
        } throws: { ($0 as? PDFError)?.code == .invalidInput }
      }
      let ink = try await annotations.create(InkAnnotationDraft(inkList: [Self.stroke]), on: page)
      await #expect {
        try await annotations.update(ink, #"{"strokeWidht": 2}"#)
      } throws: { ($0 as? PDFError)?.code == .invalidInput }
    }

    /// Every key a full typed draft writes comes back in the engine's read
    /// of the annotation it created, with the value written. A typed key the
    /// engine accepted but ignored, or read back under another name, fails
    /// here; new members are covered once a full draft above sets them.
    @Test func everyTypedKeyRoundTripsThroughTheEngine() async throws {
      let (page, annotations) = try await opened()
      let drafts: [any AnnotationDraft] = [Self.fullInk, Self.fullHighlight, Self.fullNote]
      for draft in drafts {
        let written = try object(try draft.json())
        let ref = try await annotations.create(draft, on: page)
        let read = try object(try #require(try await annotations.get(ref)).fieldsJSON)
        for (key, value) in written {
          let readValue = try #require(read[key], "the engine read back no '\(key)' for \(written["subtype"]!)")
          #expect(sameJSON(value, readValue), "'\(key)': wrote \(value), read \(readValue)")
        }
      }
    }

    // ── the JSON-string overloads ───────────────────────────────────────

    @Test func theStringOverloadsTakeTheTypedJSONAlike() async throws {
      let (page, annotations) = try await opened()
      let drafts: [any AnnotationDraft] = [Self.fullInk, Self.fullHighlight, Self.fullNote]
      for draft in drafts {
        let typed = try #require(try await annotations.get(annotations.create(draft, on: page)))
        let untyped = try #require(
          try await annotations.get(annotations.create(try draft.json(), on: page)))
        expectNoDifference(untyped.details, typed.details)
        expectNoDifference(untyped.rect, typed.rect)
        expectNoDifference(untyped.contents, typed.contents)
      }

      let patch = InkAnnotationPatch(color: "#00ff00", strokeWidth: 6, rotation: .null)
      let typed = try await annotations.create(Self.fullInk, on: page)
      let untyped = try await annotations.create(Self.fullInk, on: page)
      try await annotations.update(typed, patch)
      try await annotations.update(untyped, try patch.json())
      let untypedDetails = try await annotations.get(untyped)?.details
      let typedDetails = try await annotations.get(typed)?.details
      expectNoDifference(untypedDetails, typedDetails)

      try await annotations.tools.updateDefaults(try patch.json(), for: .ink)
      let viaString = try await annotations.tools.defaults(for: .ink)
      try await annotations.tools.updateDefaults(InkAnnotationPatch(strokeWidth: 1), for: .ink)
      try await annotations.tools.updateDefaults(patch, for: .ink)
      let viaTyped = try await annotations.tools.defaults(for: .ink)
      expectNoDifference(viaTyped, viaString)
    }
  }
}

/// `AnnotationInfo`'s subtype members, which have no public initializer,
/// as values a test can spell.
struct InkSeen: Equatable {
  var color: String?
  var opacity: Double
  var strokeWidth: Double
  var borderStyle: String
  var dashArray: [Double]?
  var inkList: [[Point]]
  var intent: String?
  var rotation: Double?
}

extension InkSeen {
  /// The points are rounded to a thousandth of a point: a rotated ink's
  /// strokes come back off by about 1e-5, the engine turning them into its
  /// unrotated frame and back.
  init(_ ink: AnnotationInfo.Ink) {
    self.init(
      color: ink.color, opacity: ink.opacity, strokeWidth: ink.strokeWidth,
      borderStyle: ink.borderStyle, dashArray: ink.dashArray,
      inkList: ink.inkList.map { $0.map(roundedPoint) }, intent: ink.intent,
      rotation: ink.rotation)
  }
}

func roundedPoint(_ point: Point) -> Point {
  Point(x: (point.x * 1000).rounded() / 1000, y: (point.y * 1000).rounded() / 1000)
}

/// JSON values equal, numbers within a thousandth (see `InkSeen`).
func sameJSON(_ left: Any, _ right: Any) -> Bool {
  switch (left, right) {
  case let (left as [String: Any], right as [String: Any]):
    left.count == right.count
      && left.allSatisfy { key, value in right[key].map { sameJSON(value, $0) } ?? false }
  case let (left as [Any], right as [Any]):
    left.count == right.count && zip(left, right).allSatisfy(sameJSON)
  case let (left as NSNumber, right as NSNumber)
  where CFGetTypeID(left) != CFBooleanGetTypeID() && CFGetTypeID(right) != CFBooleanGetTypeID():
    abs(left.doubleValue - right.doubleValue) < 0.001
  default:
    NSDictionary(dictionary: ["value": left]).isEqual(to: ["value": right])
  }
}

struct HighlightSeen: Equatable {
  var color: String
  var opacity: Double
  var quadPoints: [Quad]
}

extension HighlightSeen {
  init(_ highlight: AnnotationInfo.Highlight) {
    self.init(color: highlight.color, opacity: highlight.opacity, quadPoints: highlight.quadPoints)
  }
}

struct NoteSeen: Equatable {
  var color: String
  var opacity: Double
  var icon: String
  var open: Bool
  var state: String?
  var stateModel: String?
}

extension NoteSeen {
  init(_ note: AnnotationInfo.Note) {
    self.init(
      color: note.color, opacity: note.opacity, icon: note.icon, open: note.open,
      state: note.state, stateModel: note.stateModel)
  }
}
