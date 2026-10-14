import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct StampTests {
    func started() async throws -> (PDFViewer, StampCapability) {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, annotation: AnnotationConfig(), stamp: StampConfig()))
      try await viewer.start()
      return (viewer, try await viewer.stamps())
    }

    /// A viewer with the shipped English library imported, and its first asset.
    func imported() async throws -> (PDFViewer, StampCapability, String, StampAsset) {
      let (viewer, stamps) = try await started()
      let library = try await stamps.importLibrary(Fixture.data("stamps"))
      let asset = try #require(try await stamps.assets(StampAssetFilter(libraryId: library)).first)
      return (viewer, stamps, library, asset)
    }

    /// Run `body` in a task cancelled at once: it either finishes or throws
    /// `operation-cancelled`.
    func expectCancelled<T: Sendable>(_ body: @escaping @Sendable () async throws -> T) async {
      let task = Task { try await body() }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
    }

    @Test func aLibraryImportsWithPreviews() async throws {
      let (_, stamps, library, asset) = try await imported()
      let listed = try await stamps.library(library)
      expectNoDifference(listed?.assetIds.first, asset.id)
      expectNoDifference(asset.kind, .stamp)
      let preview = try #require(try await stamps.assetPreview(asset.id))
      expectNoDifference(preview.mimeType, "image/png")
      #expect(preview.image != nil)
    }

    @Test func aPlainPDFImportsOneAssetPerPage() async throws {
      let (_, stamps) = try await started()
      let library = try await stamps.importLibrary(Fixture.data("colors"))
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      let pages = try await viewer.pages(of: try await viewer.open(Fixture.data("colors")))
      let assets = try await stamps.assets(StampAssetFilter(libraryId: library))
      expectNoDifference(assets.count, pages.count)
    }

    @Test func importingAPNGThrows() async throws {
      let (_, stamps) = try await started()
      let png = Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
      await #expect {
        _ = try await stamps.importLibrary(png)
      } throws: { error in
        (error as? PDFError)?.code == "InvalidArg"
      }
      let count = try await stamps.libraries().count
      expectNoDifference(count, 0)
    }

    @Test func aPlacedStampIsAnAnnotation() async throws {
      let (viewer, stamps, _, asset) = try await imported()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      let ref = try await stamps.place(asset.id, in: document, on: page, at: Point(x: 100, y: 100))
      let annotations = try await viewer.annotations(for: document)
      let placed = try await annotations.get(ref)
      #expect(placed != nil)
    }

    @Test func placingIntoAClosedDocumentThrows() async throws {
      let (viewer, stamps, _, asset) = try await imported()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      try await viewer.close(document)
      await #expect(throws: PDFError.self) {
        _ = try await stamps.place(asset.id, in: document, on: page, at: Point(x: 10, y: 10))
      }
      let armed = try await stamps.armedAsset(in: document)
      expectNoDifference(armed, nil)
    }

    @Test func armingIsObservedAndDisarmingClearsIt() async throws {
      let (viewer, stamps, _, asset) = try await imported()
      let document = try await viewer.open(Fixture.data("report"))
      let events = stamps.events
      try await stamps.arm(asset.id, in: document)
      for await event in events {
        guard case let .armChanged(documentId, assetId) = event else { continue }
        expectNoDifference(documentId, document.id)
        expectNoDifference(assetId, asset.id)
        break
      }
      let armed = try await stamps.armedAsset(in: document)
      expectNoDifference(armed?.id, asset.id)
      try await stamps.disarm(in: document)
      let disarmed = try await stamps.armedAsset(in: document)
      expectNoDifference(disarmed, nil)
    }

    @Test func createAssetFromAnnotationsIsUnsupported() async throws {
      let (viewer, stamps) = try await started()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      await #expect {
        _ = try await stamps.createAsset(from: [], in: document, on: page)
      } throws: { error in
        (error as? PDFError)?.code == "unsupported"
      }
    }

    @Test func aLibraryPersistsThroughAFileStore() async throws {
      let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
      defer { try? FileManager.default.removeItem(at: directory) }
      let store = try FileStampLibraryStore(directory: directory)
      do {
        let (_, stamps) = try await started()
        let persistence = stamps.persist(to: store, debounce: .zero)
        _ = try await stamps.importLibrary(Fixture.data("stamps"))
        try await Task.sleep(for: .milliseconds(200))
        persistence.stop()
      }
      let (_, again) = try await started()
      let restored = try await again.restore(from: store)
      expectNoDifference(restored.count, 1)
      let assets = try await again.assets(StampAssetFilter(libraryId: restored.first))
      #expect(!assets.isEmpty)
    }

    @Test func aStoreThatThrowsFailsTheRestore() async throws {
      let file = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
      defer { try? FileManager.default.removeItem(at: file) }
      let store = try FileStampLibraryStore(directory: file)
      // A file where the store expects its directory.
      try FileManager.default.removeItem(at: file)
      try Data().write(to: file)
      let (_, stamps) = try await started()
      await #expect {
        _ = try await stamps.restore(from: store)
      } throws: { error in
        (error as? PDFError)?.name == "HostError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, stamps) = try await started()
      let task = Task { for await _ in stamps.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await stamps.importLibrary(Fixture.data("stamps"))
    }

    @Test func destroyingTheViewerFinishesTheEvents() async throws {
      let (viewer, stamps) = try await started()
      let events = Task { for await _ in stamps.events {} }
      try await Task.sleep(for: .milliseconds(50))
      try await viewer.destroy()
      await events.value
    }

    @Test func aDestroyedViewersCapabilityThrows() async throws {
      let (viewer, stamps) = try await started()
      try await viewer.destroy()
      await #expect {
        _ = try await stamps.libraries()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    // One per member that takes a `CancelToken`.

    @Test func importLibraryCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps) = try await started()
      let data = try Fixture.data("stamps")
      await expectCancelled { try await stamps.importLibrary(data) }
      _ = try await stamps.libraries()
    }

    @Test func createLibraryCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps) = try await started()
      await expectCancelled { try await stamps.createLibrary("Mine") }
    }

    @Test func updateLibraryCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps, library, _) = try await imported()
      await expectCancelled { try await stamps.updateLibrary(library, StampLibraryPatch(name: "Renamed")) }
    }

    @Test func deleteLibraryCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps, library, _) = try await imported()
      await expectCancelled { try await stamps.deleteLibrary(library) }
    }

    @Test func exportLibraryCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps, library, _) = try await imported()
      await expectCancelled { try await stamps.exportLibrary(library) }
    }

    @Test func renderAssetPreviewCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps, _, asset) = try await imported()
      await expectCancelled { try await stamps.renderAssetPreview(asset.id, width: 64) }
    }

    @Test func createAssetCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps, library, _) = try await imported()
      let source = try Fixture.data("colors")
      await expectCancelled {
        try await stamps.createAsset(StampAddAssetInput(libraryId: library, source: source))
      }
    }

    @Test func createAssetFromAnnotationsCancelsWhenTheTaskIsCancelled() async throws {
      let (viewer, stamps) = try await started()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      await expectCancelled { try await stamps.createAsset(from: [], in: document, on: page) }
    }

    @Test func updateAssetCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps, _, asset) = try await imported()
      await expectCancelled { try await stamps.updateAsset(asset.id, StampAssetPatch(label: "Renamed")) }
    }

    @Test func deleteAssetCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps, _, asset) = try await imported()
      await expectCancelled { try await stamps.deleteAsset(asset.id) }
    }

    @Test func moveAssetCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps, _, asset) = try await imported()
      let other = try await stamps.createLibrary("Other")
      await expectCancelled { try await stamps.moveAsset(asset.id, to: other) }
    }

    @Test func duplicateAssetCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps, _, asset) = try await imported()
      await expectCancelled { try await stamps.duplicateAsset(asset.id) }
    }

    @Test func armCancelsWhenTheTaskIsCancelled() async throws {
      let (viewer, stamps, _, asset) = try await imported()
      let document = try await viewer.open(Fixture.data("report"))
      await expectCancelled { try await stamps.arm(asset.id, in: document) }
    }

    @Test func placeCancelsWhenTheTaskIsCancelled() async throws {
      let (viewer, stamps, _, asset) = try await imported()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      await expectCancelled { try await stamps.place(asset.id, in: document, on: page, at: Point(x: 10, y: 10)) }
    }

    @Test func placeOnPagesCancelsWhenTheTaskIsCancelled() async throws {
      let (viewer, stamps, _, asset) = try await imported()
      let document = try await viewer.open(Fixture.data("report"))
      await expectCancelled {
        try await stamps.place(asset.id, in: document, onPages: nil, at: Point(x: 10, y: 10))
      }
    }

    @Test func restoreCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps) = try await started()
      let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
      defer { try? FileManager.default.removeItem(at: directory) }
      let store = try FileStampLibraryStore(directory: directory)
      await expectCancelled { try await stamps.restore(from: store) }
    }
  }
}
