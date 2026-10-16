import CustomDump
import EmbedPDF
import Foundation
import Testing

/// Answers every prompt with `tiny.png`, and counts the prompts.
final class TinyPngPicker: AnnotationFilePicker, @unchecked Sendable {
  let png: Data
  private let lock = NSLock()
  private var asked = 0

  init(png: Data) {
    self.png = png
  }

  var prompts: Int {
    lock.withLock { asked }
  }

  func pick(request: AnnotationFilePromptRequest) async -> AnnotationPickedFile? {
    lock.withLock { asked += 1 }
    return AnnotationPickedFile(bytes: png, name: "tiny.png", mimeType: "image/png")
  }
}

extension BaseSuite {
  @Suite struct AnnotationTests {
    func opened(_ fixture: String = "annotation_kinds") async throws
      -> (PDFViewer, DocumentInfo, AnnotationCapability)
    {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, annotation: AnnotationConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data(fixture))
      let annotations = try await viewer.annotations(for: document)
      try await annotations.refresh()
      return (viewer, document, annotations)
    }

    func tinyPng() throws -> Data {
      let url = try #require(
        Bundle.module.url(forResource: "tiny", withExtension: "png", subdirectory: "Fixtures"))
      return try Data(contentsOf: url)
    }

    @Test func hydrationListsTheFixturesAnnotations() async throws {
      let (_, _, annotations) = try await opened()
      // `refresh` during the hydration the open started answers when that
      // one settles and runs once more after it (`rehydrate`), so wait for
      // the second run's resync.
      let events = annotations.events
      if try await annotations.hydration() == .loading {
        for await event in events {
          if case .resynced = event { break }
        }
      }
      let hydration = try await annotations.hydration()
      expectNoDifference(hydration, .complete)
      let all = try await annotations.list()
      #expect(!all.isEmpty)
    }

    @Test func createUpdateDeleteIsObserved() async throws {
      let (viewer, document, annotations) = try await opened("report")
      let page = try await viewer.pages(of: document)[0].page
      let events = annotations.events
      let created = try await annotations.create(
        CreateAnnotationInput(
          page: page, subtype: "square",
          bounds: Rect(x: 72, y: 72, width: 100, height: 50)))
      try await annotations.update(created, AnnotationPatch(contents: "From Swift"))
      let read = try await annotations.get(created)
      expectNoDifference(read?.contents, "From Swift")
      try await annotations.delete(created)
      var sawCreated = false
      for await event in events {
        if case .created = event { sawCreated = true }
        if case .deleted = event { break }
      }
      #expect(sawCreated)
    }

    @Test func aStampFromPngBytesIsCreated() async throws {
      let (viewer, document, annotations) = try await opened("report")
      let page = try await viewer.pages(of: document)[0].page
      let stamp = try await annotations.drafts.placeStamp(
        AnnotationStampSource(bytes: try tinyPng(), mimeType: "image/png"),
        at: AnnotationStampPlacement(page: page, at: Point(x: 200, y: 200), targetWidth: 80))
      let read = try await annotations.get(stamp)
      expectNoDifference(read?.subtype, "stamp")
    }

    @Test func aSwiftFilePickerSuppliesARequestedStamp() async throws {
      let (viewer, document, annotations) = try await opened("report")
      let page = try await viewer.pages(of: document)[0].page
      let picker = TinyPngPicker(png: try tinyPng())
      try await annotations.setFilePicker(picker)
      try await viewer.interaction(for: document).activateTool("stamp")
      let asked = try await annotations.drafts.requestStamp(at: Point(x: 200, y: 200), on: page)
      #expect(asked)
      // The pick answers off the kernel thread and the stamp is placed
      // after it. A placed stamp is selected, not announced as `.created`
      // (the TypeScript's stamp path doesn't emit it either), so poll.
      let deadline = ContinuousClock.now + .seconds(5)
      var subtypes: [String] = []
      while subtypes.isEmpty, ContinuousClock.now < deadline {
        try await Task.sleep(for: .milliseconds(20))
        subtypes = try await annotations.list().map(\.subtype)
      }
      expectNoDifference(subtypes, ["stamp"])
      expectNoDifference(picker.prompts, 1)
    }

    @Test func selectingAndClearing() async throws {
      let (_, _, annotations) = try await opened()
      let first = try #require(try await annotations.list().first)
      try await annotations.selection.select([first.annotationRef])
      let refs = try await annotations.selection.refs()
      expectNoDifference(refs, [first.annotationRef])
      try await annotations.selection.clear()
      let cleared = try await annotations.selection.refs()
      expectNoDifference(cleared, [])
    }

    @Test func createFromSelectionMarksTheSelectedText() async throws {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, selection: true, annotation: AnnotationConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      let annotations = try await viewer.annotations(for: document)
      try await annotations.refresh()
      let selection = try await viewer.selection(for: document)
      // The page's text geometry loads on demand; without it the
      // selection has no segments to mark.
      try await selection.ensureLoaded(page)
      try await selection.selectPage(page)
      let refs = try await annotations.drafts.createFromSelection("highlight")
      #expect(!refs.isEmpty)
      let marked = try await annotations.get(try #require(refs.first))
      expectNoDifference(marked?.subtype, "highlight")
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, annotations) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await annotations.list()
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test(arguments: [-1..<3, 0..<(Int(UInt32.max) + 1)])
    func aTextSelectionOutsideUInt32ThrowsInsteadOfTrapping(range: Range<Int>) async throws {
      let (_, _, annotations) = try await opened()
      let ref = try #require(try await annotations.list().first).annotationRef
      await #expect {
        try await annotations.drafts.setTextSelection(range, in: ref)
      } throws: { error in
        (error as? PDFError)?.code == "invalid-input"
      }
    }

    @Test func statusIsAResourceStatus() async throws {
      let (_, _, annotations) = try await opened()
      // `opened` refreshes during the open's own hydration, which runs once
      // more after it: wait for that run's resync, as the hydration test does.
      let events = annotations.events
      if try await annotations.status() == .loading {
        for await event in events {
          if case .resynced = event { break }
        }
      }
      let status = try await annotations.status()
      expectNoDifference(status, .ready)
    }

    @Test func aSubCapabilityHeldAloneKeepsTheViewerAndReleasesIt() async throws {
      weak var released: PDFViewer?
      do {
        var tools: AnnotationToolsCapability?
        do {
          let (viewer, _, annotations) = try await opened()
          released = viewer
          tools = annotations.tools
        }
        // Only `tools` holds the viewer now: it still answers.
        #expect(released != nil)
        let listed = try await tools?.tools() ?? []
        #expect(!listed.isEmpty)
        tools = nil
      }
      #expect(released == nil)
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, annotations) = try await opened()
      let task = Task { for await _ in annotations.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await annotations.refresh()
    }

    @Test func refreshCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, annotations) = try await opened()
      let task = Task { try await annotations.refresh() }
      task.cancel()
      do {
        try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await annotations.list()
    }

    // One per member that takes a `CancelToken`, beyond `refresh` and
    // `create` above. Each is called in an already-cancelled task.

    /// The fixture's first annotation and its page.
    func cancellable() async throws -> (AnnotationCapability, AnnotationRef, PageRef) {
      let (_, _, annotations) = try await opened()
      let first = try #require(try await annotations.list().first)
      return (annotations, first.annotationRef, first.page)
    }

    func square(on page: PageRef) -> CreateAnnotationInput {
      CreateAnnotationInput(page: page, subtype: "square", bounds: Rect(x: 0, y: 0, width: 10, height: 10))
    }

    @Test func reloadCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, _, page) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.reload(page) }
    }

    @Test func createManyCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, _, page) = try await cancellable()
      let before = try await annotations.list().count
      await expectCancelledBeforeItStarts { try await annotations.create([square(on: page)]) }
      let after = try await annotations.list().count
      expectNoDifference(after, before)
    }

    @Test func updateCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.update(ref, AnnotationPatch(contents: "x")) }
    }

    @Test func updateManyCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.update([ref], AnnotationPatch(contents: "x")) }
    }

    @Test func setRotationCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.setRotation(ref, degrees: 90) }
    }

    @Test func rotateByCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.rotate(ref, by: 90) }
    }

    @Test func deleteCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.delete(ref) }
      let kept = try await annotations.get(ref)
      #expect(kept != nil)
    }

    @Test func deleteManyCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.delete([ref]) }
    }

    // The selection sub-capability, over a selected annotation.

    func selected() async throws -> (AnnotationSelectionCapability, AnnotationRef) {
      let (annotations, ref, _) = try await cancellable()
      try await annotations.selection.select([ref])
      return (annotations.selection, ref)
    }

    @Test func selectionUpdateCancelsWhenTheTaskIsCancelled() async throws {
      let (selection, _) = try await selected()
      await expectCancelledBeforeItStarts { try await selection.update(AnnotationPropsPatch(opacity: 0.5)) }
    }

    @Test func selectionUpdateFlagsCancelsWhenTheTaskIsCancelled() async throws {
      let (selection, _) = try await selected()
      await expectCancelledBeforeItStarts { try await selection.updateFlags(AnnotationFlagsPatch(hidden: true)) }
    }

    @Test func selectionDeleteCancelsWhenTheTaskIsCancelled() async throws {
      let (selection, _) = try await selected()
      await expectCancelledBeforeItStarts { try await selection.delete() }
      let refs = try await selection.refs()
      #expect(!refs.isEmpty)
    }

    @Test func selectionRotateByCancelsWhenTheTaskIsCancelled() async throws {
      let (selection, _) = try await selected()
      await expectCancelledBeforeItStarts { try await selection.rotate(.clockwise) }
    }

    @Test func selectionResetRotationCancelsWhenTheTaskIsCancelled() async throws {
      let (selection, _) = try await selected()
      await expectCancelledBeforeItStarts { try await selection.resetRotation() }
    }

    @Test func selectionToggleTextFormatCancelsWhenTheTaskIsCancelled() async throws {
      let (selection, _) = try await selected()
      await expectCancelledBeforeItStarts { try await selection.toggle(.bold) }
    }

    @Test func selectionGroupCancelsWhenTheTaskIsCancelled() async throws {
      let (selection, _) = try await selected()
      await expectCancelledBeforeItStarts { try await selection.group() }
    }

    @Test func selectionUngroupCancelsWhenTheTaskIsCancelled() async throws {
      let (selection, _) = try await selected()
      await expectCancelledBeforeItStarts { try await selection.ungroup() }
    }

    // The drafts sub-capability.

    @Test func finishCreationDraftCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.drafts.finishCreationDraft() }
    }

    @Test func setContentsCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.drafts.setContents("x", of: ref) }
    }

    @Test func endTextEditCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.drafts.endTextEdit() }
    }

    @Test func createFromSelectionCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, _, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.drafts.createFromSelection("highlight") }
    }

    @Test func armStampCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, _, _) = try await cancellable()
      let source = AnnotationStampSource(bytes: try tinyPng(), mimeType: "image/png")
      await expectCancelledBeforeItStarts { try await annotations.drafts.armStamp(source) }
      let armed = try await annotations.drafts.hasArmedStamp()
      expectNoDifference(armed, false)
    }

    @Test func placeStampCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, _, page) = try await cancellable()
      let source = AnnotationStampSource(bytes: try tinyPng(), mimeType: "image/png")
      await expectCancelledBeforeItStarts {
        try await annotations.drafts.placeStamp(source, at: AnnotationStampPlacement(page: page, at: Point(x: 50, y: 50)))
      }
    }

    @Test func createAttachmentCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, _, page) = try await cancellable()
      let file = AnnotationAttachmentFile(bytes: try tinyPng(), name: "tiny.png")
      await expectCancelledBeforeItStarts {
        try await annotations.drafts.createAttachment(file, at: Point(x: 50, y: 50), on: page)
      }
    }

    @Test func readAttachmentCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.drafts.readAttachment(ref) }
    }

    // The threads sub-capability.

    @Test func replyCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.threads.reply(to: ref, "x") }
    }

    @Test func threadSetTextCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.threads.setText("x", of: ref) }
    }

    @Test func threadSetStatusCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.threads.setStatus(.accepted, of: ref) }
    }

    @Test func threadSetMarkedCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.threads.setMarked(true, ref) }
    }

    @Test func threadDeleteCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.threads.delete(ref) }
    }

    @Test func deleteThreadCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.threads.deleteThread(ref) }
    }

    @Test func setLinkCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts {
        try await annotations.threads.setLink(.uri(uri: "https://example.invalid"), on: ref)
      }
    }

    @Test func clearLinkCancelsWhenTheTaskIsCancelled() async throws {
      let (annotations, ref, _) = try await cancellable()
      await expectCancelledBeforeItStarts { try await annotations.threads.clearLink(ref) }
    }

    @Test func createCancelsWhenTheTaskIsCancelled() async throws {
      let (viewer, document, annotations) = try await opened("report")
      let page = try await viewer.pages(of: document)[0].page
      let before = try await annotations.list().count
      let task = Task {
        try await annotations.create(
          CreateAnnotationInput(
            page: page, subtype: "square", bounds: Rect(x: 0, y: 0, width: 10, height: 10)))
      }
      task.cancel()
      _ = try? await task.value
      let after = try await annotations.list().count
      #expect(after == before || after == before + 1)
    }
  }
}
